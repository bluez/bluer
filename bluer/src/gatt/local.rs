//! Publish local GATT services to remove devices.

#![allow(missing_docs)]

use futures::{channel::oneshot, lock::Mutex, Future, FutureExt, Stream};
use pin_project::pin_project;
use std::{
    collections::{HashMap, HashSet},
    fmt,
    mem::take,
    num::NonZeroU16,
    pin::Pin,
    sync::Arc,
    task::Poll,
};
use strum::{Display, EnumString, IntoStaticStr};
use tokio::sync::{mpsc, watch};
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;
use zbus::zvariant::{OwnedFd, OwnedObjectPath, OwnedValue};

use super::{
    make_socket_pair, mtu_workaround, CharacteristicFlags, CharacteristicReader, CharacteristicWriter,
    DescriptorFlags, WriteOp, CHARACTERISTIC_INTERFACE,
};
use crate::{Adapter, Address, Device, Error, ErrorKind, Result, SessionInner, ERR_PREFIX, SERVICE_NAME};

pub(crate) const MANAGER_INTERFACE: &str = "org.bluez.GattManager1";

/// Link type.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Display, EnumString)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum LinkType {
    /// BR/EDR
    #[strum(serialize = "BR/EDR")]
    BrEdr,
    /// LE
    #[strum(serialize = "LE")]
    Le,
}

// ===========================================================================================
// Request error
// ===========================================================================================

/// Error response from us to a Bluetooth request.
#[derive(
    Clone, Copy, Debug, displaydoc::Display, Eq, PartialEq, Ord, PartialOrd, Hash, IntoStaticStr, Default,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum ReqError {
    /// Bluetooth request failed
    #[default]
    Failed,
    /// Bluetooth request already in progress
    InProgress,
    /// Invalid offset for Bluetooth GATT property
    InvalidOffset,
    /// Invalid value length for Bluetooth GATT property
    InvalidValueLength,
    /// Bluetooth request not permitted
    NotPermitted,
    /// Bluetooth request not authorized
    NotAuthorized,
    /// Bluetooth request not supported
    NotSupported,
}

impl std::error::Error for ReqError {}

impl From<ReqError> for zbus::fdo::Error {
    fn from(err: ReqError) -> Self {
        let name: &'static str = err.into();
        zbus::fdo::Error::Failed(format!("{}.{}", ERR_PREFIX, name))
    }
}

/// Result of a Bluetooth request to us.
pub type ReqResult<T> = std::result::Result<T, ReqError>;

// ===========================================================================================
// Service
// ===========================================================================================

// ----------
// Definition
// ----------

/// Definition of local GATT service exposed over Bluetooth.
#[derive(Debug, Default)]
pub struct Service {
    /// 128-bit service UUID.
    pub uuid: Uuid,
    /// Service handle.
    ///
    /// Set to [None] to auto allocate an available handle.
    pub handle: Option<NonZeroU16>,
    /// Indicates whether or not this GATT service is a
    /// primary service.
    ///
    /// If false, the service is secondary.
    pub primary: bool,
    /// List of GATT characteristics to expose.
    pub characteristics: Vec<Characteristic>,
    /// Control handle for service once it has been registered.
    pub control_handle: ServiceControlHandle,
    #[doc(hidden)]
    pub _non_exhaustive: (),
}

// ----------
// Controller
// ----------

/// An object to control a service once it has been registered.
///
/// Use [service_control] to obtain controller and associated handle.
pub struct ServiceControl {
    handle_rx: watch::Receiver<Option<NonZeroU16>>,
}

impl fmt::Debug for ServiceControl {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ServiceControl {{ handle: {} }}", self.handle().map(|h| h.get()).unwrap_or_default())
    }
}

impl ServiceControl {
    /// Gets the assigned handle of the service.
    pub fn handle(&self) -> crate::Result<NonZeroU16> {
        match *self.handle_rx.borrow() {
            Some(handle) => Ok(handle),
            None => Err(Error::new(ErrorKind::NotRegistered)),
        }
    }
}

/// A handle to store inside a service definition to make it controllable
/// once it has been registered.
///
/// Use [service_control] to obtain controller and associated handle.
pub struct ServiceControlHandle {
    handle_tx: watch::Sender<Option<NonZeroU16>>,
}

impl Default for ServiceControlHandle {
    fn default() -> Self {
        Self { handle_tx: watch::channel(None).0 }
    }
}

impl fmt::Debug for ServiceControlHandle {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ServiceControlHandle")
    }
}

/// Creates a [ServiceControl] and its associated [ServiceControlHandle].
///
/// Keep the [ServiceControl] and store the [ServiceControlHandle] in [Service::control_handle].
pub fn service_control() -> (ServiceControl, ServiceControlHandle) {
    let (handle_tx, handle_rx) = watch::channel(None);
    (ServiceControl { handle_rx }, ServiceControlHandle { handle_tx })
}

// ---------------
// D-Bus interface
// ---------------

/// A service exposed over D-Bus to bluez.
pub(crate) struct RegisteredService {
    s: Service,
}

impl RegisteredService {
    fn new(s: Service) -> Self {
        if let Some(handle) = s.handle {
            let _ = s.control_handle.handle_tx.send(Some(handle));
        }
        Self { s }
    }
}

#[zbus::interface(name = "org.bluez.GattService1")]
impl RegisteredService {
    #[zbus(property, name = "UUID")]
    fn uuid(&self) -> String {
        self.s.uuid.to_string()
    }

    #[zbus(property)]
    fn primary(&self) -> bool {
        self.s.primary
    }

    #[zbus(property)]
    fn handle(&self) -> u16 {
        self.s.handle.map(|h| h.get()).unwrap_or_default()
    }

    #[zbus(property)]
    fn set_handle(&self, handle: u16) -> zbus::Result<()> {
        log::trace!("Set handle: {}", handle);
        let handle = NonZeroU16::new(handle);
        let _ = self.s.control_handle.handle_tx.send(handle);
        Ok(())
    }
}

// ===========================================================================================
// Characteristic
// ===========================================================================================

// ----------
// Definition
// ----------

/// Characteristic read value function.
pub type CharacteristicReadFun = Box<
    dyn (Fn(CharacteristicReadRequest) -> Pin<Box<dyn Future<Output = ReqResult<Vec<u8>>> + Send>>) + Send + Sync,
>;

/// Characteristic read definition.
#[derive(custom_debug::Debug)]
pub struct CharacteristicRead {
    /// If set allows clients to read this characteristic.
    pub read: bool,
    /// Require encryption.
    pub encrypt_read: bool,
    /// Require authentication.
    pub encrypt_authenticated_read: bool,
    /// Require security.
    pub secure_read: bool,
    /// Function called for each read request returning value.
    #[debug(skip)]
    pub fun: CharacteristicReadFun,
    #[doc(hidden)]
    pub _non_exhaustive: (),
}

impl Default for CharacteristicRead {
    fn default() -> Self {
        Self {
            read: false,
            encrypt_read: false,
            encrypt_authenticated_read: false,
            secure_read: false,
            fun: Box::new(|_| async move { Err(ReqError::NotSupported) }.boxed()),
            _non_exhaustive: (),
        }
    }
}

impl CharacteristicRead {
    fn set_characteristic_flags(&self, f: &mut CharacteristicFlags) {
        f.read = self.read;
        f.encrypt_read = self.encrypt_read;
        f.encrypt_authenticated_read = self.encrypt_authenticated_read;
        f.secure_read = self.secure_read;
    }
}

/// Characteristic write value function.
pub type CharacteristicWriteFun = Box<
    dyn Fn(Vec<u8>, CharacteristicWriteRequest) -> Pin<Box<dyn Future<Output = ReqResult<()>> + Send>>
        + Send
        + Sync,
>;

/// Characteristic write value method.
pub enum CharacteristicWriteMethod {
    /// Call specified function for each write request.
    Fun(CharacteristicWriteFun),
    /// Provide written data over asynchronous IO functions.
    /// This has low overhead.
    ///
    /// Use [CharacteristicControl] to obtain reader.
    Io,
}

impl fmt::Debug for CharacteristicWriteMethod {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Fun(_) => write!(f, "Fun"),
            Self::Io => write!(f, "Io"),
        }
    }
}

impl Default for CharacteristicWriteMethod {
    fn default() -> Self {
        Self::Fun(Box::new(|_, _| async move { Err(ReqError::NotSupported) }.boxed()))
    }
}

/// Characteristic write definition.
#[derive(Debug, Default)]
pub struct CharacteristicWrite {
    /// If set allows clients to use the Write Command ATT operation.
    pub write: bool,
    /// If set allows clients to use the Write Request/Response operation.
    pub write_without_response: bool,
    /// If set allows clients to use the Reliable Writes procedure.
    pub reliable_write: bool,
    /// If set allows clients to use the Signed Write Without Response procedure.
    pub authenticated_signed_writes: bool,
    /// Require encryption.
    pub encrypt_write: bool,
    /// Require authentication.
    pub encrypt_authenticated_write: bool,
    /// Require security.
    pub secure_write: bool,
    /// Write value method.
    pub method: CharacteristicWriteMethod,
    #[doc(hidden)]
    pub _non_exhaustive: (),
}

impl CharacteristicWrite {
    fn set_characteristic_flags(&self, f: &mut CharacteristicFlags) {
        f.write = self.write;
        f.write_without_response = self.write_without_response;
        f.reliable_write = self.reliable_write;
        f.authenticated_signed_writes = self.authenticated_signed_writes;
        f.encrypt_write = self.encrypt_write;
        f.encrypt_authenticated_write = self.encrypt_authenticated_write;
        f.secure_write = self.secure_write;
    }
}

/// Characteristic start notifications function.
///
/// This function cannot fail, since there is to way to provide an error response to the
/// requesting device.
pub type CharacteristicNotifyFun =
    Box<dyn Fn(CharacteristicNotifier) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

/// Characteristic notify value method.
pub enum CharacteristicNotifyMethod {
    /// Call specified function when client starts a notification session.
    Fun(CharacteristicNotifyFun),
    /// Write notify data over asynchronous IO.
    /// This has low overhead.
    ///
    /// Use [CharacteristicControl] to obtain writer.
    Io,
}

impl Default for CharacteristicNotifyMethod {
    fn default() -> Self {
        Self::Fun(Box::new(|_| async move {}.boxed()))
    }
}

impl fmt::Debug for CharacteristicNotifyMethod {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Fun(_) => write!(f, "Fun"),
            Self::Io => write!(f, "Io"),
        }
    }
}

/// Characteristic notify definition.
#[derive(Debug, Default)]
pub struct CharacteristicNotify {
    /// If set allows the client to use the Handle Value Notification operation.
    pub notify: bool,
    /// If set allows the client to use the Handle Value Indication/Confirmation operation.
    ///
    /// Confirmations will only be provided when this is [true] and [notify](Self::notify) is [false].
    pub indicate: bool,
    /// Notification and indication method.
    pub method: CharacteristicNotifyMethod,
    #[doc(hidden)]
    pub _non_exhaustive: (),
}

impl CharacteristicNotify {
    fn set_characteristic_flags(&self, f: &mut CharacteristicFlags) {
        f.notify = self.notify;
        f.indicate = self.indicate;
    }
}

/// Definition of local GATT characteristic exposed over Bluetooth.
#[derive(Default, Debug)]
pub struct Characteristic {
    /// 128-bit characteristic UUID.
    pub uuid: Uuid,
    /// Characteristic handle.
    ///
    /// Set to [None] to auto allocate an available handle.
    pub handle: Option<NonZeroU16>,
    /// If set, permits broadcasts of the Characteristic Value using
    /// Server Characteristic Configuration Descriptor.
    pub broadcast: bool,
    /// If set a client can write to the Characteristic User Description Descriptor.
    pub writable_auxiliaries: bool,
    /// Authorize flag.
    pub authorize: bool,
    /// Characteristic descriptors.
    pub descriptors: Vec<Descriptor>,
    /// Read value of characteristic.
    pub read: Option<CharacteristicRead>,
    /// Write value of characteristic.
    pub write: Option<CharacteristicWrite>,
    /// Notify client of characteristic value change.
    pub notify: Option<CharacteristicNotify>,
    /// Control handle for characteristic once it has been registered.
    pub control_handle: CharacteristicControlHandle,
    #[doc(hidden)]
    pub _non_exhaustive: (),
}

impl Characteristic {
    fn set_characteristic_flags(&self, f: &mut CharacteristicFlags) {
        f.broadcast = self.broadcast;
        f.writable_auxiliaries = self.writable_auxiliaries;
        f.authorize = self.authorize;
    }
}

// ------------------
// Callback interface
// ------------------

/// Parse the `device` option.
fn parse_device(dict: &HashMap<String, OwnedValue>) -> zbus::fdo::Result<(String, Address)> {
    let path = dict.get("device").ok_or_else(|| zbus::fdo::Error::InvalidArgs("device missing".to_string()))?;
    let path: OwnedObjectPath = (*path)
        .try_clone()
        .map_err(|_| zbus::fdo::Error::InvalidArgs("clone failed".to_string()))?
        .try_into()
        .map_err(|_| zbus::fdo::Error::InvalidArgs("device invalid".to_string()))?;
    let (adapter, addr) = Device::parse_dbus_path(&path).ok_or_else(|| {
        log::warn!("cannot parse device path: {}", path);
        zbus::fdo::Error::InvalidArgs("device path invalid".to_string())
    })?;
    Ok((adapter.to_string(), addr))
}

/// Read value request.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct CharacteristicReadRequest {
    /// Name of adapter making this request.
    pub adapter_name: String,
    /// Address of device making this request.
    pub device_address: Address,
    /// Offset.
    pub offset: u16,
    /// Exchanged MTU.
    pub mtu: u16,
    /// Link type.
    pub link: Option<LinkType>,
}

impl CharacteristicReadRequest {
    fn from_dict(dict: &HashMap<String, OwnedValue>) -> zbus::fdo::Result<Self> {
        let (adapter_name, device_address) = parse_device(dict)?;

        let offset = dict
            .get("offset")
            .map(|v| {
                (*v).try_clone()
                    .map_err(|_| zbus::fdo::Error::InvalidArgs("clone failed".to_string()))?
                    .try_into()
                    .map_err(|_| zbus::fdo::Error::InvalidArgs("offset invalid".to_string()))
            })
            .transpose()?
            .unwrap_or_default();

        let mtu = dict
            .get("mtu")
            .ok_or_else(|| zbus::fdo::Error::InvalidArgs("mtu missing".to_string()))
            .and_then(|v| {
                (*v).try_clone()
                    .map_err(|_| zbus::fdo::Error::InvalidArgs("clone failed".to_string()))?
                    .try_into()
                    .map_err(|_| zbus::fdo::Error::InvalidArgs("mtu invalid".to_string()))
            })?;

        let link = dict
            .get("link")
            .map(|v| {
                (*v).try_clone()
                    .map_err(|_| zbus::fdo::Error::InvalidArgs("clone failed".to_string()))?
                    .try_into()
                    .map_err(|_| zbus::fdo::Error::InvalidArgs("link invalid".to_string()))
            })
            .transpose()?
            .and_then(|v: String| v.parse().ok());

        Ok(Self { adapter_name, device_address, offset, mtu, link })
    }
}

/// Write value request.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct CharacteristicWriteRequest {
    /// Name of adapter making this request.
    pub adapter_name: String,
    /// Address of device making this request.
    pub device_address: Address,
    /// Start offset.
    pub offset: u16,
    /// Write operation type.
    pub op_type: WriteOp,
    /// Exchanged MTU.
    pub mtu: u16,
    /// Link type.
    pub link: Option<LinkType>,
    /// True if prepare authorization request.
    pub prepare_authorize: bool,
}

impl CharacteristicWriteRequest {
    fn from_dict(dict: &HashMap<String, OwnedValue>) -> zbus::fdo::Result<Self> {
        let (adapter_name, device_address) = parse_device(dict)?;

        let offset = dict
            .get("offset")
            .map(|v| {
                (*v).try_clone()
                    .map_err(|_| zbus::fdo::Error::InvalidArgs("clone failed".to_string()))?
                    .try_into()
                    .map_err(|_| zbus::fdo::Error::InvalidArgs("offset invalid".to_string()))
            })
            .transpose()?
            .unwrap_or_default();

        let op_type = dict
            .get("type")
            .map(|v| {
                (*v).try_clone()
                    .map_err(|_| zbus::fdo::Error::InvalidArgs("clone failed".to_string()))?
                    .try_into()
                    .map_err(|_| zbus::fdo::Error::InvalidArgs("type invalid".to_string()))
            })
            .transpose()?
            .map(|s: String| s.parse().map_err(|_| zbus::fdo::Error::InvalidArgs("type invalid".to_string())))
            .transpose()?
            .unwrap_or_default();

        let mtu = dict
            .get("mtu")
            .ok_or_else(|| zbus::fdo::Error::InvalidArgs("mtu missing".to_string()))
            .and_then(|v| {
                (*v).try_clone()
                    .map_err(|_| zbus::fdo::Error::InvalidArgs("clone failed".to_string()))?
                    .try_into()
                    .map_err(|_| zbus::fdo::Error::InvalidArgs("mtu invalid".to_string()))
            })?;

        let link = dict
            .get("link")
            .map(|v| {
                (*v).try_clone()
                    .map_err(|_| zbus::fdo::Error::InvalidArgs("clone failed".to_string()))?
                    .try_into()
                    .map_err(|_| zbus::fdo::Error::InvalidArgs("link invalid".to_string()))
            })
            .transpose()?
            .and_then(|v: String| v.parse().ok());

        let prepare_authorize = dict
            .get("prepare-authorize")
            .map(|v| {
                (*v).try_clone()
                    .map_err(|_| zbus::fdo::Error::InvalidArgs("clone failed".to_string()))?
                    .try_into()
                    .map_err(|_| zbus::fdo::Error::InvalidArgs("prepare-authorize invalid".to_string()))
            })
            .transpose()?
            .unwrap_or_default();

        Ok(Self { adapter_name, device_address, offset, op_type, mtu, link, prepare_authorize })
    }
}

/// Notification session.
///
/// Use this to send notifications or indications.
pub struct CharacteristicNotifier {
    connection: zbus::Connection,
    path: OwnedObjectPath,
    stop_notify_tx: mpsc::Sender<()>,
    confirm_rx: Option<mpsc::Receiver<()>>,
}

impl CharacteristicNotifier {
    /// True, if each notification is confirmed by the receiving device.
    ///
    /// This is the case when the Indication mechanism is used.
    pub fn confirming(&self) -> bool {
        self.confirm_rx.is_some()
    }

    /// True, if the notification session has been stopped by the receiving device.
    pub fn is_stopped(&self) -> bool {
        self.stop_notify_tx.is_closed()
    }

    /// Resolves once the notification session has been stopped by the receiving device.
    pub fn stopped(&self) -> impl Future<Output = ()> {
        let stop_notify_tx = self.stop_notify_tx.clone();
        async move { stop_notify_tx.closed().await }
    }

    /// Sends a notification or indication with the specified data to the receiving device.
    ///
    /// If [confirming](Self::confirming) is true, the function waits until a confirmation is received from
    /// the device before it returns.
    ///
    /// This fails when the notification session has been stopped by the receiving device.
    pub async fn notify(&mut self, value: Vec<u8>) -> Result<()> {
        if self.is_stopped() {
            return Err(Error::new(ErrorKind::NotificationSessionStopped));
        }

        // Flush confirmation queue.
        // This is necessary because previous notify call could have been aborted
        // before receiving the confirmation.
        if let Some(confirm_rx) = &mut self.confirm_rx {
            while let Some(Some(())) = confirm_rx.recv().now_or_never() {}
        }

        // Send notification.
        let changed_properties: HashMap<String, OwnedValue> =
            [("Value".to_string(), zbus::zvariant::Value::from(value).try_into().expect("value conversion"))]
                .into_iter()
                .collect();
        let invalidated_properties: Vec<String> = Vec::new();

        self.connection
            .emit_signal(
                Option::<&str>::None,
                &self.path,
                "org.freedesktop.DBus.Properties",
                "PropertiesChanged",
                &(CHARACTERISTIC_INTERFACE, changed_properties, invalidated_properties),
            )
            .await
            .map_err(|_| Error::new(ErrorKind::NotificationSessionStopped))?;

        // Wait for confirmation if this is an indication session.
        // Note that we can be aborted before we receive the confirmation.
        if let Some(confirm_rx) = &mut self.confirm_rx {
            match confirm_rx.recv().await {
                Some(()) => Ok(()),
                None => Err(Error::new(ErrorKind::IndicationUnconfirmed)),
            }
        } else {
            Ok(())
        }
    }
}

// ------------
// IO interface
// ------------

/// A remote request to start writing to a characteristic via IO.
pub struct CharacteristicWriteIoRequest {
    adapter_name: String,
    device_address: Address,
    mtu: u16,
    link: Option<LinkType>,
    tx: oneshot::Sender<ReqResult<OwnedFd>>,
}

impl CharacteristicWriteIoRequest {
    /// Name of adapter making this request.
    pub fn adapter_name(&self) -> &str {
        &self.adapter_name
    }

    /// Address of device making this request.
    pub fn device_address(&self) -> Address {
        self.device_address
    }

    /// Maximum transmission unit.
    pub fn mtu(&self) -> usize {
        self.mtu.into()
    }

    /// Link type.
    pub fn link(&self) -> Option<LinkType> {
        self.link
    }

    /// Accept the write request.
    pub fn accept(self) -> Result<CharacteristicReader> {
        let CharacteristicWriteIoRequest { adapter_name, device_address, mtu, tx, .. } = self;
        let (fd, socket) = make_socket_pair(false)?;
        let _ = tx.send(Ok(fd));
        Ok(CharacteristicReader { adapter_name, device_address, mtu: mtu.into(), socket, buf: Vec::new() })
    }

    /// Reject the write request.
    pub fn reject(self, reason: ReqError) {
        let _ = self.tx.send(Err(reason));
    }
}

// ----------
// Controller
// ----------

/// An event on a published characteristic.
pub enum CharacteristicControlEvent {
    /// A remote request to start writing via IO.
    ///
    /// This event occurs only when using [CharacteristicWriteMethod::Io].
    Write(CharacteristicWriteIoRequest),
    /// A remote request to start notifying via IO.
    ///
    /// Note that BlueZ acknowledges the client's request before notifying us
    /// of the start of the notification session.
    ///
    /// This event occurs only when using [CharacteristicNotifyMethod::Io].
    Notify(CharacteristicWriter),
}

impl fmt::Debug for CharacteristicControlEvent {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Write(_) => write!(f, "Write"),
            Self::Notify(_) => write!(f, "Notify"),
        }
    }
}

/// An object to control a characteristic and receive events once it has been registered.
///
/// Use [characteristic_control] to obtain controller and associated handle.
#[pin_project]
pub struct CharacteristicControl {
    handle_rx: watch::Receiver<Option<NonZeroU16>>,
    #[pin]
    events_rx: ReceiverStream<CharacteristicControlEvent>,
}

impl fmt::Debug for CharacteristicControl {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "CharacteristicControl {{ handle: {} }}", self.handle().map(|h| h.get()).unwrap_or_default())
    }
}

impl CharacteristicControl {
    /// Gets the assigned handle of the characteristic.
    pub fn handle(&self) -> crate::Result<NonZeroU16> {
        match *self.handle_rx.borrow() {
            Some(handle) => Ok(handle),
            None => Err(Error::new(ErrorKind::NotRegistered)),
        }
    }
}

impl Stream for CharacteristicControl {
    type Item = CharacteristicControlEvent;

    fn poll_next(self: Pin<&mut Self>, cx: &mut std::task::Context) -> Poll<Option<Self::Item>> {
        self.project().events_rx.poll_next(cx)
    }
}

/// A handle to store inside a characteristic definition to make it controllable
/// once it has been registered.
///
/// Use [characteristic_control] to obtain controller and associated handle.
pub struct CharacteristicControlHandle {
    handle_tx: watch::Sender<Option<NonZeroU16>>,
    events_tx: mpsc::Sender<CharacteristicControlEvent>,
}

impl Default for CharacteristicControlHandle {
    fn default() -> Self {
        Self { handle_tx: watch::channel(None).0, events_tx: mpsc::channel(1).0 }
    }
}

impl fmt::Debug for CharacteristicControlHandle {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "CharacteristicControlHandle")
    }
}

/// Creates a [CharacteristicControl] and its associated [CharacteristicControlHandle].
///
/// Keep the [CharacteristicControl] and store the [CharacteristicControlHandle] in [Characteristic::control_handle].
pub fn characteristic_control() -> (CharacteristicControl, CharacteristicControlHandle) {
    let (handle_tx, handle_rx) = watch::channel(None);
    let (events_tx, events_rx) = mpsc::channel(1);
    (
        CharacteristicControl { handle_rx, events_rx: ReceiverStream::new(events_rx) },
        CharacteristicControlHandle { handle_tx, events_tx },
    )
}

// ---------------
// D-Bus interface
// ---------------

/// Characteristic acquire write or notify request.
#[derive(Debug, Clone)]
#[non_exhaustive]
struct CharacteristicAcquireRequest {
    /// Name of adapter making this request.
    pub adapter_name: String,
    /// Address of device making this request.
    pub device_address: Address,
    /// Exchanged MTU.
    pub mtu: u16,
    /// Link type.
    pub link: Option<LinkType>,
}

impl CharacteristicAcquireRequest {
    fn from_dict(dict: &HashMap<String, OwnedValue>) -> zbus::fdo::Result<Self> {
        let (adapter_name, device_address) = parse_device(dict)?;

        let mtu = dict
            .get("mtu")
            .ok_or_else(|| zbus::fdo::Error::InvalidArgs("mtu missing".to_string()))
            .and_then(|v| {
                (*v).try_clone()
                    .map_err(|_| zbus::fdo::Error::InvalidArgs("clone failed".to_string()))?
                    .try_into()
                    .map_err(|_| zbus::fdo::Error::InvalidArgs("mtu invalid".to_string()))
            })?;

        let link = dict
            .get("link")
            .map(|v| {
                (*v).try_clone()
                    .map_err(|_| zbus::fdo::Error::InvalidArgs("clone failed".to_string()))?
                    .try_into()
                    .map_err(|_| zbus::fdo::Error::InvalidArgs("link invalid".to_string()))
            })
            .transpose()?
            .and_then(|v: String| v.parse().ok());

        Ok(Self { adapter_name, device_address, mtu, link })
    }
}

/// Notification state of a registered characteristic.
struct CharacteristicNotifyState {
    confirm_tx: Option<mpsc::Sender<()>>,
    _stop_notify_rx: mpsc::Receiver<()>,
}

/// A characteristic exposed over D-Bus to bluez.
pub(crate) struct RegisteredCharacteristic {
    c: Characteristic,
    notify: Mutex<Option<CharacteristicNotifyState>>,
    service_path: OwnedObjectPath,
}

impl RegisteredCharacteristic {
    fn new(c: Characteristic, service_path: OwnedObjectPath) -> Self {
        if let Some(handle) = c.handle {
            let _ = c.control_handle.handle_tx.send(Some(handle));
        }
        Self { c, notify: Mutex::new(None), service_path }
    }
}

#[zbus::interface(name = "org.bluez.GattCharacteristic1")]
impl RegisteredCharacteristic {
    #[zbus(property, name = "UUID")]
    fn uuid(&self) -> String {
        self.c.uuid.to_string()
    }

    #[zbus(property)]
    fn service(&self) -> OwnedObjectPath {
        self.service_path.clone()
    }

    #[zbus(property)]
    fn flags(&self) -> Vec<String> {
        let mut flags = CharacteristicFlags::default();
        self.c.set_characteristic_flags(&mut flags);
        if let Some(read) = &self.c.read {
            read.set_characteristic_flags(&mut flags);
        }
        if let Some(write) = &self.c.write {
            write.set_characteristic_flags(&mut flags);
        }
        if let Some(notify) = &self.c.notify {
            notify.set_characteristic_flags(&mut flags);
        }
        flags.as_vec()
    }

    #[zbus(property)]
    fn handle(&self) -> u16 {
        self.c.handle.map(|h| h.get()).unwrap_or_default()
    }

    #[zbus(property)]
    fn set_handle(&self, handle: u16) -> zbus::Result<()> {
        log::trace!("Set handle: {}", handle);
        let handle = NonZeroU16::new(handle);
        let _ = self.c.control_handle.handle_tx.send(handle);
        Ok(())
    }

    #[zbus(property)]
    fn write_acquired(&self) -> bool {
        match &self.c.write {
            Some(CharacteristicWrite { method: CharacteristicWriteMethod::Io, .. }) => false,
            _ => false,
        }
    }

    #[zbus(property, name = "NotifyAcquired")]
    fn notify_acquired(&self) -> bool {
        match &self.c.notify {
            Some(CharacteristicNotify { method: CharacteristicNotifyMethod::Io, .. }) => false,
            _ => false,
        }
    }

    async fn read_value(&self, options: HashMap<String, OwnedValue>) -> zbus::fdo::Result<Vec<u8>> {
        let options = CharacteristicReadRequest::from_dict(&options)?;
        match &self.c.read {
            Some(read) => {
                let value = (read.fun)(options).await?;
                Ok(value)
            }
            None => Err(ReqError::NotSupported.into()),
        }
    }

    async fn write_value(&self, value: Vec<u8>, options: HashMap<String, OwnedValue>) -> zbus::fdo::Result<()> {
        let options = CharacteristicWriteRequest::from_dict(&options)?;
        match &self.c.write {
            Some(CharacteristicWrite { method: CharacteristicWriteMethod::Fun(fun), .. }) => {
                fun(value, options).await?;
                Ok(())
            }
            _ => Err(ReqError::NotSupported.into()),
        }
    }

    async fn start_notify(
        &self, #[zbus(object_server)] _server: &zbus::ObjectServer,
        #[zbus(signal_context)] ctxt: zbus::SignalContext<'_>,
    ) -> zbus::fdo::Result<()> {
        let path = ctxt.path().to_owned();
        let connection = ctxt.connection().clone();
        match &self.c.notify {
            Some(CharacteristicNotify {
                method: CharacteristicNotifyMethod::Fun(notify_fn),
                indicate,
                notify,
                _non_exhaustive: (),
            }) => {
                let (stop_notify_tx, stop_notify_rx) = mpsc::channel(1);
                let (confirm_tx, confirm_rx) = if *indicate && !*notify {
                    let (tx, rx) = mpsc::channel(1);
                    (Some(tx), Some(rx))
                } else {
                    (None, None)
                };
                {
                    let mut notify = self.notify.lock().await;
                    *notify = Some(CharacteristicNotifyState { _stop_notify_rx: stop_notify_rx, confirm_tx });
                }
                let notifier =
                    CharacteristicNotifier { connection, path: path.into(), stop_notify_tx, confirm_rx };
                notify_fn(notifier).await;
                Ok(())
            }
            _ => Err(ReqError::NotSupported.into()),
        }
    }

    async fn stop_notify(&self) -> zbus::fdo::Result<()> {
        let mut notify = self.notify.lock().await;
        *notify = None;
        Ok(())
    }

    async fn confirm(&self) -> zbus::fdo::Result<()> {
        let mut notify = self.notify.lock().await;
        if let Some(CharacteristicNotifyState { confirm_tx: Some(confirm_tx), .. }) = &mut *notify {
            let _ = confirm_tx.send(()).await;
        }
        Ok(())
    }

    async fn acquire_write(&self, options: HashMap<String, OwnedValue>) -> zbus::fdo::Result<(OwnedFd, u16)> {
        let options = CharacteristicAcquireRequest::from_dict(&options)?;
        match &self.c.write {
            Some(CharacteristicWrite { method: CharacteristicWriteMethod::Io, .. }) => {
                let (tx, rx) = oneshot::channel();
                let req = CharacteristicWriteIoRequest {
                    adapter_name: options.adapter_name.clone(),
                    device_address: options.device_address,
                    mtu: options.mtu,
                    link: options.link,
                    tx,
                };
                self.c
                    .control_handle
                    .events_tx
                    .send(CharacteristicControlEvent::Write(req))
                    .await
                    .map_err(|_| ReqError::Failed)?;
                let fd = rx.await.map_err(|_| ReqError::Failed)??;
                Ok((fd, options.mtu))
            }
            _ => Err(ReqError::NotSupported.into()),
        }
    }

    async fn acquire_notify(&self, options: HashMap<String, OwnedValue>) -> zbus::fdo::Result<(OwnedFd, u16)> {
        let options = CharacteristicAcquireRequest::from_dict(&options)?;
        match &self.c.notify {
            Some(CharacteristicNotify { method: CharacteristicNotifyMethod::Io, .. }) => {
                let (fd, socket) = make_socket_pair(true).map_err(|_| ReqError::Failed)?;
                let mtu = mtu_workaround(options.mtu.into());
                let writer = CharacteristicWriter {
                    adapter_name: options.adapter_name.clone(),
                    device_address: options.device_address,
                    mtu,
                    socket,
                };
                let _ = self.c.control_handle.events_tx.send(CharacteristicControlEvent::Notify(writer)).await;
                Ok((fd, options.mtu))
            }
            _ => Err(ReqError::NotSupported.into()),
        }
    }
}

// ===========================================================================================
// Characteristic descriptor
// ===========================================================================================

// ----------
// Definition
// ----------

/// Characteristic descriptor read value function.
pub type DescriptorReadFun =
    Box<dyn Fn(DescriptorReadRequest) -> Pin<Box<dyn Future<Output = ReqResult<Vec<u8>>> + Send>> + Send + Sync>;

/// Characteristic descriptor read definition.
#[derive(custom_debug::Debug)]
pub struct DescriptorRead {
    /// If set allows clients to read this characteristic descriptor.
    pub read: bool,
    /// Require encryption.
    pub encrypt_read: bool,
    /// Require authentication.
    pub encrypt_authenticated_read: bool,
    /// Require security.
    pub secure_read: bool,
    /// Function called for each read request returning value.
    #[debug(skip)]
    pub fun: DescriptorReadFun,
    #[doc(hidden)]
    pub _non_exhaustive: (),
}

impl Default for DescriptorRead {
    fn default() -> Self {
        Self {
            read: false,
            encrypt_read: false,
            encrypt_authenticated_read: false,
            secure_read: false,
            fun: Box::new(|_| async move { Err(ReqError::NotSupported) }.boxed()),
            _non_exhaustive: (),
        }
    }
}

impl DescriptorRead {
    fn set_descriptor_flags(&self, f: &mut DescriptorFlags) {
        f.read = self.read;
        f.encrypt_read = self.encrypt_read;
        f.encrypt_authenticated_read = self.encrypt_authenticated_read;
        f.secure_read = self.secure_read;
    }
}

/// Characteristic descriptor write value function.
pub type DescriptorWriteFun = Box<
    dyn Fn(Vec<u8>, DescriptorWriteRequest) -> Pin<Box<dyn Future<Output = ReqResult<()>> + Send>> + Send + Sync,
>;

/// Characteristic descriptor write definition.
#[derive(custom_debug::Debug)]
pub struct DescriptorWrite {
    /// If set allows clients to use the Write Command ATT operation.
    pub write: bool,
    /// Require encryption.
    pub encrypt_write: bool,
    /// Require authentication.
    pub encrypt_authenticated_write: bool,
    /// Require security.
    pub secure_write: bool,
    /// Function called for each write request.
    #[debug(skip)]
    pub fun: DescriptorWriteFun,
    #[doc(hidden)]
    pub _non_exhaustive: (),
}

impl Default for DescriptorWrite {
    fn default() -> Self {
        Self {
            write: false,
            encrypt_write: false,
            encrypt_authenticated_write: false,
            secure_write: false,
            fun: Box::new(|_, _| async move { Err(ReqError::NotSupported) }.boxed()),
            _non_exhaustive: (),
        }
    }
}

impl DescriptorWrite {
    fn set_descriptor_flags(&self, f: &mut DescriptorFlags) {
        f.write = self.write;
        f.encrypt_write = self.encrypt_write;
        f.encrypt_authenticated_write = self.encrypt_authenticated_write;
        f.secure_write = self.secure_write;
    }
}

/// Definition of local GATT characteristic descriptor exposed over Bluetooth.
#[derive(Default, Debug)]
pub struct Descriptor {
    /// 128-bit descriptor UUID.
    pub uuid: Uuid,
    /// Characteristic descriptor handle.
    ///
    /// Set to [None] to auto allocate an available handle.
    pub handle: Option<NonZeroU16>,
    /// Authorize flag.
    pub authorize: bool,
    /// Read value of characteristic descriptor.
    pub read: Option<DescriptorRead>,
    /// Write value of characteristic descriptor.
    pub write: Option<DescriptorWrite>,
    /// Control handle for characteristic descriptor once it has been registered.
    pub control_handle: DescriptorControlHandle,
    #[doc(hidden)]
    pub _non_exhaustive: (),
}

impl Descriptor {
    fn set_descriptor_flags(&self, f: &mut DescriptorFlags) {
        f.authorize = self.authorize;
    }
}

// ------------------
// Callback interface
// ------------------

/// Read characteristic descriptor value request.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct DescriptorReadRequest {
    /// Name of adapter making this request.
    pub adapter_name: String,
    /// Address of device making this request.
    pub device_address: Address,
    /// Offset.
    pub offset: u16,
    /// Link type.
    pub link: Option<LinkType>,
}

impl DescriptorReadRequest {
    fn from_dict(dict: &HashMap<String, OwnedValue>) -> zbus::fdo::Result<Self> {
        let (adapter_name, device_address) = parse_device(dict)?;

        let offset = dict
            .get("offset")
            .map(|v| {
                (*v).try_clone()
                    .map_err(|_| zbus::fdo::Error::InvalidArgs("clone failed".to_string()))?
                    .try_into()
                    .map_err(|_| zbus::fdo::Error::InvalidArgs("offset invalid".to_string()))
            })
            .transpose()?
            .unwrap_or_default();

        let link = dict
            .get("link")
            .map(|v| {
                (*v).try_clone()
                    .map_err(|_| zbus::fdo::Error::InvalidArgs("clone failed".to_string()))?
                    .try_into()
                    .map_err(|_| zbus::fdo::Error::InvalidArgs("link invalid".to_string()))
            })
            .transpose()?
            .and_then(|v: String| v.parse().ok());

        Ok(Self { adapter_name, device_address, offset, link })
    }
}

/// Write characteristic descriptor value request.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct DescriptorWriteRequest {
    /// Name of adapter making this request.
    pub adapter_name: String,
    /// Address of device making this request.
    pub device_address: Address,
    /// Offset.
    pub offset: u16,
    /// Link type.
    pub link: Option<LinkType>,
    /// Is prepare authorization request?
    pub prepare_authorize: bool,
}

impl DescriptorWriteRequest {
    fn from_dict(dict: &HashMap<String, OwnedValue>) -> zbus::fdo::Result<Self> {
        let (adapter_name, device_address) = parse_device(dict)?;

        let offset = dict
            .get("offset")
            .map(|v| {
                (*v).try_clone()
                    .map_err(|_| zbus::fdo::Error::InvalidArgs("clone failed".to_string()))?
                    .try_into()
                    .map_err(|_| zbus::fdo::Error::InvalidArgs("offset invalid".to_string()))
            })
            .transpose()?
            .unwrap_or_default();

        let link = dict
            .get("link")
            .map(|v| {
                (*v).try_clone()
                    .map_err(|_| zbus::fdo::Error::InvalidArgs("clone failed".to_string()))?
                    .try_into()
                    .map_err(|_| zbus::fdo::Error::InvalidArgs("link invalid".to_string()))
            })
            .transpose()?
            .and_then(|v: String| v.parse().ok());

        let prepare_authorize = dict
            .get("prepare-authorize")
            .ok_or_else(|| zbus::fdo::Error::InvalidArgs("prepare-authorize missing".to_string()))
            .and_then(|v| {
                (*v).try_clone()
                    .map_err(|_| zbus::fdo::Error::InvalidArgs("clone failed".to_string()))?
                    .try_into()
                    .map_err(|_| zbus::fdo::Error::InvalidArgs("prepare-authorize invalid".to_string()))
            })?;

        Ok(Self { adapter_name, device_address, offset, link, prepare_authorize })
    }
}

// ----------
// Controller
// ----------

/// An object to control a characteristic descriptor once it has been registered.
///
/// Use [descriptor_control] to obtain controller and associated handle.
pub struct DescriptorControl {
    handle_rx: watch::Receiver<Option<NonZeroU16>>,
}

impl fmt::Debug for DescriptorControl {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "DescriptorControl {{ handle: {} }}", self.handle().map(|h| h.get()).unwrap_or_default())
    }
}

impl DescriptorControl {
    /// Gets the assigned handle of the characteristic descriptor.
    pub fn handle(&self) -> crate::Result<NonZeroU16> {
        match *self.handle_rx.borrow() {
            Some(handle) => Ok(handle),
            None => Err(Error::new(ErrorKind::NotRegistered)),
        }
    }
}

/// A handle to store inside a characteristic descriptors definition to make
/// it controllable once it has been registered.
///
/// Use [descriptor_control] to obtain controller and associated handle.
pub struct DescriptorControlHandle {
    handle_tx: watch::Sender<Option<NonZeroU16>>,
}

impl Default for DescriptorControlHandle {
    fn default() -> Self {
        Self { handle_tx: watch::channel(None).0 }
    }
}

impl fmt::Debug for DescriptorControlHandle {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "DescriptorControlHandle")
    }
}

/// Creates a [DescriptorControl] and its associated [DescriptorControlHandle].
///
/// Keep the [DescriptorControl] and store the [DescriptorControlHandle] in [Descriptor::control_handle].
pub fn descriptor_control() -> (DescriptorControl, DescriptorControlHandle) {
    let (handle_tx, handle_rx) = watch::channel(None);
    (DescriptorControl { handle_rx }, DescriptorControlHandle { handle_tx })
}

// ---------------
// D-Bus interface
// ---------------

/// A characteristic descriptor exposed over D-Bus to bluez.
pub(crate) struct RegisteredDescriptor {
    d: Descriptor,
    characteristic_path: OwnedObjectPath,
}

impl RegisteredDescriptor {
    fn new(d: Descriptor, characteristic_path: OwnedObjectPath) -> Self {
        if let Some(handle) = d.handle {
            let _ = d.control_handle.handle_tx.send(Some(handle));
        }
        Self { d, characteristic_path }
    }
}

#[zbus::interface(name = "org.bluez.GattDescriptor1")]
impl RegisteredDescriptor {
    #[zbus(property, name = "UUID")]
    fn uuid(&self) -> String {
        self.d.uuid.to_string()
    }

    #[zbus(property)]
    fn characteristic(&self) -> OwnedObjectPath {
        self.characteristic_path.clone()
    }

    #[zbus(property)]
    fn flags(&self) -> Vec<String> {
        let mut flags = DescriptorFlags::default();
        self.d.set_descriptor_flags(&mut flags);
        if let Some(read) = &self.d.read {
            read.set_descriptor_flags(&mut flags);
        }
        if let Some(write) = &self.d.write {
            write.set_descriptor_flags(&mut flags);
        }
        flags.as_vec()
    }

    #[zbus(property)]
    fn handle(&self) -> u16 {
        self.d.handle.map(|h| h.get()).unwrap_or_default()
    }

    #[zbus(property)]
    fn set_handle(&self, handle: u16) -> zbus::Result<()> {
        log::trace!("Set handle: {}", handle);
        let handle = NonZeroU16::new(handle);
        let _ = self.d.control_handle.handle_tx.send(handle);
        Ok(())
    }

    async fn read_value(&self, flags: HashMap<String, OwnedValue>) -> zbus::fdo::Result<Vec<u8>> {
        let options = DescriptorReadRequest::from_dict(&flags)?;
        match &self.d.read {
            Some(read) => {
                let value = (read.fun)(options).await?;
                Ok(value)
            }
            None => Err(ReqError::NotSupported.into()),
        }
    }

    async fn write_value(&self, value: Vec<u8>, flags: HashMap<String, OwnedValue>) -> zbus::fdo::Result<()> {
        let options = DescriptorWriteRequest::from_dict(&flags)?;
        match &self.d.write {
            Some(write) => {
                (write.fun)(value, options).await?;
                Ok(())
            }
            None => Err(ReqError::NotSupported.into()),
        }
    }
}

// ===========================================================================================
// Application
// ===========================================================================================

pub(crate) const GATT_APP_PREFIX: &str = "/org/bluez/gatt/app/";

/// Definition of local GATT application to publish over Bluetooth.
#[derive(Debug, Default)]
pub struct Application {
    /// Services to publish.
    pub services: Vec<Service>,
    #[doc(hidden)]
    pub _non_exhaustive: (),
}

impl Application {
    pub(crate) async fn register(
        mut self, inner: Arc<SessionInner>, adapter_name: Arc<String>,
    ) -> crate::Result<ApplicationHandle> {
        type CleanupAction = Box<dyn FnOnce(zbus::Connection) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send>;
        let mut cleanup_actions: Vec<CleanupAction> = Vec::new();
        let app_path_str = format!("{}{}", GATT_APP_PREFIX, Uuid::new_v4().as_simple());
        let app_path = OwnedObjectPath::try_from(app_path_str.clone()).unwrap();
        log::trace!("Publishing application at {}", &app_path);

        let server = inner.connection.object_server();

        let object_manager = zbus::fdo::ObjectManager;
        server.at(&app_path, object_manager).await?;
        let app_path_clone = app_path.clone();
        cleanup_actions.push(Box::new(move |connection| {
            Box::pin(async move {
                let server = connection.object_server();
                let _ = server.remove::<zbus::fdo::ObjectManager, _>(&app_path_clone).await;
            })
        }));

        let services = take(&mut self.services);

        for (service_idx, mut service) in services.into_iter().enumerate() {
            let chars = take(&mut service.characteristics);

            let reg_service = RegisteredService::new(service);
            let service_path_str = format!("{}/service{}", &app_path_str, service_idx);
            let service_path = OwnedObjectPath::try_from(service_path_str.clone()).unwrap();
            log::trace!("Publishing service at {}", &service_path);
            server.at(&service_path, reg_service).await?;
            let service_path_clone = service_path.clone();
            cleanup_actions.push(Box::new(move |connection| {
                Box::pin(async move {
                    let server = connection.object_server();
                    let _ = server.remove::<RegisteredService, _>(&service_path_clone).await;
                })
            }));

            for (char_idx, mut char) in chars.into_iter().enumerate() {
                let descs = take(&mut char.descriptors);

                let reg_char = RegisteredCharacteristic::new(char, service_path.clone());
                let char_path_str = format!("{}/char{}", &service_path_str, char_idx);
                let char_path = OwnedObjectPath::try_from(char_path_str.clone()).unwrap();
                log::trace!("Publishing characteristic at {}", &char_path);
                server.at(&char_path, reg_char).await?;
                let char_path_clone = char_path.clone();
                cleanup_actions.push(Box::new(move |connection| {
                    Box::pin(async move {
                        let server = connection.object_server();
                        let _ = server.remove::<RegisteredCharacteristic, _>(&char_path_clone).await;
                    })
                }));

                for (desc_idx, desc) in descs.into_iter().enumerate() {
                    let reg_desc = RegisteredDescriptor::new(desc, char_path.clone());
                    let desc_path_str = format!("{}/desc{}", &char_path_str, desc_idx);
                    let desc_path = OwnedObjectPath::try_from(desc_path_str.clone()).unwrap();
                    log::trace!("Publishing descriptor at {}", &desc_path);
                    server.at(&desc_path, reg_desc).await?;
                    let desc_path_clone = desc_path.clone();
                    cleanup_actions.push(Box::new(move |connection| {
                        Box::pin(async move {
                            let server = connection.object_server();
                            let _ = server.remove::<RegisteredDescriptor, _>(&desc_path_clone).await;
                        })
                    }));
                }
            }
        }

        log::trace!("Registering application at {}", &app_path);
        let proxy = zbus::Proxy::new(
            &inner.connection,
            SERVICE_NAME,
            Adapter::dbus_path(&adapter_name)?,
            MANAGER_INTERFACE,
        )
        .await?;
        let () =
            proxy.call("RegisterApplication", &(app_path.clone(), HashMap::<String, OwnedValue>::new())).await?;

        let (drop_tx, drop_rx) = oneshot::channel();
        let app_path_unreg = app_path.clone();
        let connection = inner.connection.clone();
        tokio::spawn(async move {
            let _ = drop_rx.await;

            log::trace!("Unregistering application at {}", &app_path_unreg);
            let proxy = zbus::Proxy::new(
                &connection,
                SERVICE_NAME,
                Adapter::dbus_path(&adapter_name).unwrap(),
                MANAGER_INTERFACE,
            )
            .await;
            if let Ok(proxy) = proxy {
                let _: zbus::Result<()> = proxy.call("UnregisterApplication", &(app_path_unreg,)).await;
            }

            for action in cleanup_actions.into_iter().rev() {
                action(connection.clone()).await;
            }
        });

        Ok(ApplicationHandle { name: app_path, _drop_tx: drop_tx })
    }
}

/// Handle to local GATT application published over Bluetooth.
///
/// Drop this handle to unpublish.
pub struct ApplicationHandle {
    name: OwnedObjectPath,
    _drop_tx: oneshot::Sender<()>,
}

impl Drop for ApplicationHandle {
    fn drop(&mut self) {
        // required for drop order
    }
}

impl fmt::Debug for ApplicationHandle {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ApplicationHandle {{ {} }}", &self.name)
    }
}

// ===========================================================================================
// GATT profile
// ===========================================================================================

pub(crate) const GATT_PROFILE_PREFIX: &str = "/org/bluez/gatt/profile/";

/// Definition of local profile (GATT client) instance.
///
/// By registering this type of object
/// an application effectively indicates support for a specific GATT profile
/// and requests automatic connections to be established to devices
/// supporting it.
#[derive(Debug, Clone, Default)]
pub struct Profile {
    /// 128-bit GATT service UUIDs to auto connect.
    pub uuids: HashSet<Uuid>,
    #[doc(hidden)]
    pub _non_exhaustive: (),
}

#[zbus::interface(name = "org.bluez.GattProfile1")]
impl Profile {
    /// UUIDs.
    #[zbus(property, name = "UUIDs")]
    fn uuids(&self) -> Vec<String> {
        self.uuids.iter().map(|uuid| uuid.to_string()).collect()
    }
}

impl Profile {
    pub(crate) async fn register(
        self, inner: Arc<SessionInner>, adapter_name: Arc<String>,
    ) -> crate::Result<ProfileHandle> {
        let profile_path_str = format!("{}{}", GATT_PROFILE_PREFIX, Uuid::new_v4().as_simple());
        let profile_path = OwnedObjectPath::try_from(profile_path_str.clone()).unwrap();
        log::trace!("Publishing profile at {}", &profile_path);

        let server = inner.connection.object_server();
        server.at(&profile_path, self).await?;

        log::trace!("Registering profile at {}", &profile_path);
        let proxy = zbus::Proxy::new(
            &inner.connection,
            SERVICE_NAME,
            Adapter::dbus_path(&adapter_name)?,
            MANAGER_INTERFACE,
        )
        .await?;
        let () = proxy
            .call("RegisterApplication", &(profile_path.clone(), HashMap::<String, OwnedValue>::new()))
            .await?;

        let (drop_tx, drop_rx) = oneshot::channel();
        let profile_path_unreg = profile_path.clone();
        let connection = inner.connection.clone();
        tokio::spawn(async move {
            let _ = drop_rx.await;

            log::trace!("Unregistering profile at {}", &profile_path_unreg);
            let proxy = zbus::Proxy::new(
                &connection,
                SERVICE_NAME,
                Adapter::dbus_path(&adapter_name).unwrap(),
                MANAGER_INTERFACE,
            )
            .await;
            if let Ok(proxy) = proxy {
                let _: zbus::Result<()> =
                    proxy.call("UnregisterApplication", &(profile_path_unreg.clone(),)).await;
            }

            log::trace!("Unpublishing profile at {}", &profile_path_unreg);
            let server = connection.object_server();
            let _ = server.remove::<Self, _>(&profile_path_unreg).await;
        });

        Ok(ProfileHandle { name: profile_path, _drop_tx: drop_tx })
    }
}

/// Handle to published local profile (GATT client) instance.
///
/// Drop this handle to unpublish.
#[must_use = "ProfileHandle must be held for profile to be published"]
pub struct ProfileHandle {
    name: OwnedObjectPath,
    _drop_tx: oneshot::Sender<()>,
}

impl Drop for ProfileHandle {
    fn drop(&mut self) {
        // required for drop order
    }
}

impl fmt::Debug for ProfileHandle {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ProfileHandle {{ {} }}", &self.name)
    }
}
