//! Local GATT services.

use dbus::{
    arg::{AppendAll, OwnedFd, PropMap},
    nonblock::{Proxy, SyncConnection},
    MethodErr,
};
use dbus_crossroads::{Context, Crossroads, IfaceBuilder, IfaceToken};
use futures::stream::StreamExt;
use futures::{
    channel::{mpsc, oneshot},
    lock::Mutex,
    Future, SinkExt,
};
use libc::{c_int, socketpair, AF_LOCAL, SOCK_CLOEXEC, SOCK_NONBLOCK, SOCK_SEQPACKET};
use pin_project::{pin_project, pinned_drop};
use std::{
    collections::HashSet,
    fmt,
    marker::PhantomData,
    mem::take,
    num::NonZeroU16,
    os::unix::{io::RawFd, prelude::FromRawFd},
    pin::Pin,
    sync::{Arc, Weak},
    task::Poll,
    time::Duration,
};
use strum::IntoStaticStr;
use tokio::{
    io::{self, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::UnixStream,
    sync::watch,
    time::sleep,
};
use uuid::Uuid;

use super::{CharacteristicDescriptorFlags, CharacteristicFlags, WriteValueType};
use crate::{
    make_socket_pair, parent_path, Adapter, Error, LinkType, Result, SessionInner, ERR_PREFIX, SERVICE_NAME,
    TIMEOUT,
};

pub(crate) const MANAGER_INTERFACE: &str = "org.bluez.GattManager1";

/// Error response from us to a Bluetooth request.
#[derive(Clone, Copy, Debug, Error, IntoStaticStr)]
pub enum Reject {
    #[error("Bluetooth request failed")]
    Failed,
    #[error("Bluetooth request already in progress")]
    InProgress,
    #[error("Invalid offset for Bluetooth GATT property")]
    InvalidOffset,
    #[error("Invalid value length for Bluetooth GATT property")]
    InvalidValueLength,
    #[error("Bluetooth request not permitted")]
    NotPermitted,
    #[error("Bluetooth request not authorized")]
    NotAuthorized,
    #[error("Bluetooth request not supported")]
    NotSupported,
}

impl Default for Reject {
    fn default() -> Self {
        Self::Failed
    }
}

impl From<Reject> for dbus::MethodErr {
    fn from(err: Reject) -> Self {
        let name: &'static str = err.clone().into();
        Self::from((ERR_PREFIX.to_string() + name, &err.to_string()))
    }
}

/// Result of a Bluetooth request to us.
pub type ReqResult<T> = std::result::Result<T, Reject>;

/// Call method on Arc D-Bus object we are serving.
fn method_call<T: Send + Sync + 'static, R: AppendAll, F: Future<Output = ReqResult<R>> + Send + 'static>(
    mut ctx: Context, cr: &mut Crossroads, f: impl FnOnce(Arc<T>) -> F,
) -> impl Future<Output = PhantomData<R>> {
    let data_ref: &mut Arc<T> = cr.data_mut(ctx.path()).unwrap();
    let data: Arc<T> = data_ref.clone();
    async move {
        let result = f(data).await;
        ctx.reply(result.into())
    }
}

// ===========================================================================================
// Service
// ===========================================================================================

/// Local GATT service exposed over Bluetooth.
#[derive(Debug)]
pub struct Service {
    /// 128-bit service UUID.
    pub uuid: Uuid,
    /// Indicates whether or not this GATT service is a
    /// primary service.
    ///
    /// If false, the service is secondary.
    pub primary: bool,
    /// List of GATT characteristics to expose.
    pub characteristics: Vec<Characteristic>,
}

impl Service {
    pub(crate) fn register_interface(cr: &mut Crossroads) -> IfaceToken<Arc<Self>> {
        cr.register("org.bluez.GattService1", |ib: &mut IfaceBuilder<Arc<Self>>| {
            cr_property!(ib, "UUID", s => {
                Some(s.uuid.to_string())
            });
            cr_property!(ib, "Primary", s => {
                Some(s.primary)
            });
        })
    }
}

// ===========================================================================================
// Characteristic
// ===========================================================================================

/// Characteristic read flags.
#[derive(Default, Debug, Clone, Copy, Eq, PartialEq)]
pub struct CharacteristicReadFlags {
    /// If set allows clients to read this characteristic.
    pub read: bool,
    /// Require encryption.
    pub encrypt_read: bool,
    /// Require authentication.
    pub encrypt_authenticated_read: bool,
    /// Require security.
    pub secure_read: bool,
}

impl CharacteristicReadFlags {
    pub fn set_characteristic_flags(&self, f: &mut CharacteristicFlags) {
        f.read = self.read;
        f.encrypt_read = self.encrypt_read;
        f.encrypt_authenticated_read = self.encrypt_authenticated_read;
        f.secure_read = self.secure_read;
    }
}

impl From<CharacteristicFlags> for CharacteristicReadFlags {
    fn from(f: CharacteristicFlags) -> Self {
        Self {
            read: f.read,
            encrypt_read: f.encrypt_read,
            encrypt_authenticated_read: f.encrypt_authenticated_read,
            secure_read: f.secure_read,
        }
    }
}

/// Characteristic read value function.
pub type CharacteristicReadFn = Box<
    dyn (Fn(ReadCharacteristicValueRequest) -> Pin<Box<dyn Future<Output = ReqResult<Vec<u8>>> + Send>>)
        + Send
        + Sync,
>;

/// Characteristic read.
pub struct CharacteristicRead {
    /// Function called for each read request returning value.
    pub fun: CharacteristicReadFn,
    /// Flags.
    pub flags: CharacteristicReadFlags,
}

impl fmt::Debug for CharacteristicRead {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "CharacteristicRead {{ fun, flags: {:?} }}", &self.flags)
    }
}

/// Characteristic write flags.
#[derive(Default, Debug, Clone, Copy, Eq, PartialEq)]
pub struct CharacteristicWriteFlags {
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
}

impl CharacteristicWriteFlags {
    pub fn set_characteristic_flags(&self, f: &mut CharacteristicFlags) {
        f.write = self.write;
        f.write_without_response = self.write_without_response;
        f.reliable_write = self.reliable_write;
        f.authenticated_signed_writes = self.authenticated_signed_writes;
        f.encrypt_write = self.encrypt_write;
        f.encrypt_authenticated_write = self.encrypt_authenticated_write;
        f.secure_write = self.secure_write;
    }
}

impl From<CharacteristicFlags> for CharacteristicWriteFlags {
    fn from(f: CharacteristicFlags) -> Self {
        Self {
            write: f.write,
            write_without_response: f.write_without_response,
            reliable_write: f.reliable_write,
            authenticated_signed_writes: f.authenticated_signed_writes,
            encrypt_write: f.encrypt_write,
            encrypt_authenticated_write: f.encrypt_authenticated_write,
            secure_write: f.secure_write,
        }
    }
}

/// Characteristic write.
#[derive(Debug)]
pub struct CharacteristicWrite {
    /// Write value method.
    pub method: CharacteristicWriteMethod,
    /// Flags.
    pub flags: CharacteristicWriteFlags,
}

/// Characteristic write value function.
pub type CharacteristicWriteFn = Box<
    dyn Fn(Vec<u8>, WriteCharacteristicValueRequest) -> Pin<Box<dyn Future<Output = ReqResult<()>> + Send>>
        + Send
        + Sync,
>;

/// Characteristic write value method.
pub enum CharacteristicWriteMethod {
    /// Call specified function for each write request.
    Fun(CharacteristicWriteFn),
    /// Provide written data over `AsyncRead` IO.
    ///
    /// Use `CharacteristicControlHandle` to obtain reader.
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

/// Characteristic notify flags.
#[derive(Default, Debug, Clone, Copy, Eq, PartialEq)]
pub struct CharacteristicNotifyFlags {
    /// If set allows the server to use the Handle Value Notification operation.
    pub notify: bool,
    /// If set allows the server to use the Handle Value Indication/Confirmation operation.
    ///
    /// Confirmations will only be provided when this is `true` and `notify` is `false`.
    pub indicate: bool,
}

impl CharacteristicNotifyFlags {
    pub fn set_characteristic_flags(&self, f: &mut CharacteristicFlags) {
        f.notify = self.notify;
        f.indicate = self.indicate;
    }
}

impl From<CharacteristicFlags> for CharacteristicNotifyFlags {
    fn from(f: CharacteristicFlags) -> Self {
        Self { notify: f.notify, indicate: f.indicate }
    }
}

/// Notification request.
#[derive(Debug)]
pub struct CharacteristicValueNotifier {
    connection: Arc<SyncConnection>,
    stop_notify_rx: watch::Receiver<bool>,
    confirm_rx: Option<mpsc::Receiver<()>>,
}

impl CharacteristicValueNotifier {
    /// True, if each notification is confirmed by the receiving device.
    ///
    /// This is the case when the Indication mechanism is used.
    pub fn confirming(&self) -> bool {
        self.confirm_rx.is_some()
    }

    /// True, if the notification session has been stopped by the receiving device.
    pub fn is_closed(&self) -> bool {
        *self.stop_notify_rx.borrow()
    }

    /// Returns a future that resolves once the notification has been stopped.
    pub fn closed(&self) -> impl Future<Output = ()> {
        let mut stop_notify_rx = self.stop_notify_rx.clone();
        async move {
            while !*stop_notify_rx.borrow() {
                if stop_notify_rx.changed().await.is_err() {
                    break;
                }
            }
        }
    }

    /// Sends a notification or indication with the specified data to the receiving device.
    ///
    /// If `confirming` is true, the function waits until a confirmation is received from
    /// the device before it returns.
    pub async fn notify(&self, value: Vec<u8>) -> Result<()> {
        todo!()
    }
}

/// Characteristic start notifications function.
///
/// This function cannot fail, since there is to way to provide an error response to the
/// requesting device.
pub type CharacteristicStartNotifyFn =
    Box<dyn Fn(CharacteristicValueNotifier) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

/// Characteristic notify value method.
#[derive(Debug)]
pub enum CharacteristicNotifyMethod {
    /// Call specified function when client starts a notification session.
    Fn(CharacteristicStartNotifyFn),
    /// Write notify data over `AsyncWrite` IO.
    ///
    /// Use `CharacteristicControlHandle` to obtain writer.
    Io,
}

/// Characteristic notify.
#[derive(Debug)]
pub struct CharacteristicNotify {
    /// Notification method.
    pub method: CharacteristicNotifyMethod,
    /// Flags.
    pub flags: CharacteristicNotifyFlags,
}

/// Characteristic flags not related to read, write and notify operations.
#[derive(Default, Debug, Clone, Copy, Eq, PartialEq)]
pub struct CharacteristicOtherFlags {
    /// If set permits broadcasts of the Characteristic Value using
    /// Server Characteristic Configuration Descriptor.
    pub broadcast: bool,
    /// If set a client can write to the Characteristic User Description Descriptor.
    pub writable_auxiliaries: bool,
    /// Authorize.
    pub authorize: bool,
}

impl CharacteristicOtherFlags {
    pub fn set_characteristic_flags(&self, f: &mut CharacteristicFlags) {
        f.broadcast = self.broadcast;
        f.writable_auxiliaries = self.writable_auxiliaries;
        f.authorize = self.authorize;
    }
}

impl From<CharacteristicFlags> for CharacteristicOtherFlags {
    fn from(f: CharacteristicFlags) -> Self {
        Self { broadcast: f.broadcast, writable_auxiliaries: f.writable_auxiliaries, authorize: f.authorize }
    }
}

/// Local GATT characteristic exposed over Bluetooth.
#[derive(Default, Debug)]
pub struct Characteristic {
    /// 128-bit characteristic UUID.
    pub uuid: Uuid,
    /// Characteristic handle.
    ///
    /// Set to `None` to auto allocate an available handle.
    pub handle: Option<NonZeroU16>,
    /// Characteristic flags unrelated to read, write and notify operations.
    pub other_flags: CharacteristicOtherFlags,
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
}

/// A remote request to start writing to a characteristic via IO.
pub struct CharacteristicWriteRequest {
    mtu: u16,
    link: Option<LinkType>,
    tx: oneshot::Sender<ReqResult<OwnedFd>>,
}

impl CharacteristicWriteRequest {
    /// Maximum transmission unit.
    pub fn mtu(&self) -> u16 {
        self.mtu
    }

    /// Link type.
    pub fn link(&self) -> Option<LinkType> {
        self.link
    }

    /// Accept the write request.
    pub fn accept(self) -> Result<CharacteristicReader> {
        let CharacteristicWriteRequest { mtu, link, tx } = self;
        let (fd, stream) = make_socket_pair()?;
        let _ = tx.send(Ok(fd));
        Ok(CharacteristicReader { mtu: mtu.into(), link, stream })
    }

    /// Reject the write request.
    pub fn reject(self, reason: Reject) {
        let _ = self.tx.send(Err(reason));
    }
}

/// Provides write requests to a characteristic as an IO stream.
#[pin_project]
pub struct CharacteristicReader {
    mtu: usize,
    link: Option<LinkType>,
    #[pin]
    stream: UnixStream,
}

impl CharacteristicReader {
    /// Maximum transmission unit.
    pub fn mtu(&self) -> usize {
        self.mtu
    }

    /// Link type.
    pub fn link(&self) -> Option<LinkType> {
        self.link
    }

    /// Gets the underlying UNIX socket.
    pub fn get(&self) -> &UnixStream {
        &self.stream
    }

    /// Gets the underlying UNIX socket mutably.
    pub fn get_mut(&mut self) -> &mut UnixStream {
        &mut self.stream
    }

    /// Transforms the reader into the underlying UNIX socket.
    pub fn into_inner(self) -> UnixStream {
        self.stream
    }
}

impl fmt::Debug for CharacteristicReader {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "CharacteristicReader {{ {:?} }}", &self.stream)
    }
}

impl AsyncRead for CharacteristicReader {
    fn poll_read(
        self: Pin<&mut Self>, cx: &mut std::task::Context, buf: &mut io::ReadBuf,
    ) -> Poll<std::io::Result<()>> {
        self.project().stream.poll_read(cx, buf)
    }
}

/// Allows sending of notifications of a characteristic via an IO stream.
#[pin_project]
pub struct CharacteristicWriter {
    mtu: usize,
    link: Option<LinkType>,
    #[pin]
    stream: UnixStream,
}

impl CharacteristicWriter {
    /// Maximum transmission unit.
    pub fn mtu(&self) -> usize {
        self.mtu
    }

    /// Link type.
    pub fn link(&self) -> Option<LinkType> {
        self.link
    }

    /// Gets the underlying UNIX socket.
    pub fn get(&self) -> &UnixStream {
        &self.stream
    }

    /// Gets the underlying UNIX socket mutably.
    pub fn get_mut(&mut self) -> &mut UnixStream {
        &mut self.stream
    }

    /// Transforms the reader into the underlying UNIX socket.
    pub fn into_inner(self) -> UnixStream {
        self.stream
    }
}

impl fmt::Debug for CharacteristicWriter {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "CharacteristicWriter {{ {:?} }}", &self.stream)
    }
}

impl AsyncWrite for CharacteristicWriter {
    fn poll_write(self: Pin<&mut Self>, cx: &mut std::task::Context, buf: &[u8]) -> Poll<std::io::Result<usize>> {
        self.project().stream.poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut std::task::Context) -> Poll<std::io::Result<()>> {
        self.project().stream.poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut std::task::Context) -> Poll<std::io::Result<()>> {
        self.project().stream.poll_shutdown(cx)
    }
}

/// An object to control a characteristic once it has been registered.
pub struct CharacteristicControl {
    handle_rx: watch::Receiver<Option<NonZeroU16>>,
    write_request_rx: mpsc::Receiver<CharacteristicWriteRequest>,
    notifier_rx: mpsc::Receiver<CharacteristicWriter>,
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
            None => Err(Error::NotRegistered),
        }
    }

    /// Gets the next request to start writing to the characteristic.
    pub async fn write_request(&mut self) -> Result<CharacteristicWriteRequest> {
        match self.write_request_rx.next().await {
            Some(req) => Ok(req),
            None => Err(Error::NotRegistered),
        }
    }

    /// Gets the next notification session.
    ///
    /// Note that bluez acknowledges the client's request before notifying us
    /// of the start of the notification session.
    pub async fn notifier(&mut self) -> Result<CharacteristicWriter> {
        match self.notifier_rx.next().await {
            Some(writer) => Ok(writer),
            None => Err(Error::NotRegistered),
        }
    }
}

/// A handle to control a characteristic once it has been registered.
pub struct CharacteristicControlHandle {
    handle_tx: watch::Sender<Option<NonZeroU16>>,
    write_request_tx: mpsc::Sender<CharacteristicWriteRequest>,
    notifier_tx: mpsc::Sender<CharacteristicWriter>,
}

impl Default for CharacteristicControlHandle {
    fn default() -> Self {
        Self {
            handle_tx: watch::channel(None).0,
            write_request_tx: mpsc::channel(0).0,
            notifier_tx: mpsc::channel(0).0,
        }
    }
}

impl fmt::Debug for CharacteristicControlHandle {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "CharacteristicControlHandle")
    }
}

/// Creates a `CharacteristicControl` and its associated handle.
pub fn characteristic_control() -> (CharacteristicControl, CharacteristicControlHandle) {
    let (handle_tx, handle_rx) = watch::channel(None);
    let (write_request_tx, write_request_rx) = mpsc::channel(0);
    let (notifier_tx, notifier_rx) = mpsc::channel(0);
    (
        CharacteristicControl { handle_rx, write_request_rx, notifier_rx },
        CharacteristicControlHandle { handle_tx, write_request_tx, notifier_tx },
    )
}

struct RegisteredCharacteristic {
    reg: Characteristic,
    // sense of this:
    // probably none, since we can send all we need
}

impl RegisteredCharacteristic {
    pub(crate) fn register_interface(cr: &mut Crossroads) -> IfaceToken<Arc<Self>> {
        cr.register("org.bluez.GattCharacteristic1", |ib: &mut IfaceBuilder<Arc<Self>>| {
            cr_property!(ib, "UUID", c => {
                Some(c.reg.uuid.to_string())
            });
            cr_property!(ib, "Flags", c => {
                let mut flags = CharacteristicFlags::default();
                c.reg.other_flags.set_characteristic_flags(&mut flags);
                if let Some(read) = &c.reg.read {
                    read.flags.set_characteristic_flags(&mut flags);
                }
                if let Some(write) = &c.reg.write {
                    write.flags.set_characteristic_flags(&mut flags);
                }
                if let Some(notify) = &c.reg.notify {
                    notify.flags.set_characteristic_flags(&mut flags);
                }
                Some(flags.to_vec())
            });
            ib.property("Service").get(|ctx, _| Ok(parent_path(ctx.path())));
            ib.property("Handle").get(|ctx, c| Ok(c.reg.handle.map(|h| h.get()).unwrap_or_default())).set(
                |ctx, c, v| {
                    let handle = NonZeroU16::new(v);
                    dbg!(&handle);
                    c.reg.control_handle.handle_tx.send(handle);
                    Ok(None)
                },
            );
            cr_property!(ib, "WriteAcquired", c => {
                if let Some(CharacteristicWrite { method: CharacteristicWriteMethod::Io, .. }) = &c.reg.write {
                    Some(false)
                } else {
                    None
                }
            });
            cr_property!(ib, "NotifyAcquired", c => {
                if let Some(CharacteristicNotify { method: CharacteristicNotifyMethod::Io, .. }) = &c.reg.notify {
                    Some(false)
                } else {
                    None
                }
            });
            ib.method_with_cr_async("ReadValue", ("options",), ("value",), |ctx, cr, (options,): (PropMap,)| {
                method_call(ctx, cr, |c: Arc<Self>| async move {
                    let options = ReadCharacteristicValueRequest::from_dict(&options)?;
                    match &c.reg.read {
                        Some(read) => {
                            let value = (read.fun)(options).await?;
                            Ok((value,))
                        }
                        None => Err(Reject::NotSupported.into()),
                    }
                })
            });
            ib.method_with_cr_async(
                "WriteValue",
                ("value", "options"),
                (),
                |ctx, cr, (value, options): (Vec<u8>, PropMap)| {
                    method_call(ctx, cr, |c: Arc<Self>| async move {
                        let options = WriteCharacteristicValueRequest::from_dict(&options)?;
                        match &c.reg.write {
                            Some(CharacteristicWrite { method: CharacteristicWriteMethod::Fun(fun), .. }) => {
                                fun(value, options).await?;
                                Ok(())
                            }
                            _ => Err(Reject::NotSupported),
                        }
                    })
                },
            );
            ib.method_with_cr_async("StartNotify", (), (), |ctx, cr, ()| {
                method_call(ctx, cr, |c: Arc<Self>| async move { 
                    match &c.reg.notify {
                        Some(CharacteristicNotify { method: CharacteristicNotifyMethod::Fn(notify_fn), .. }) => {
                            let notifier = CharacteristicValueNotifier {
                                connection: (),
                                stop_notify_rx: (),
                                confirm_rx: (),
                            };
                        }
                        _ => Err(Reject::NotSupported),
                    }
                 })
            });
            ib.method_with_cr_async("StopNotify", (), (), |ctx, cr, ()| {
                method_call(ctx, cr, |c: Arc<Self>| async move { todo!() })
            });
            ib.method_with_cr_async(
                "AcquireWrite",
                ("options",),
                ("fd", "mtu"),
                |ctx, cr, (options,): (PropMap,)| {
                    method_call(ctx, cr, |c: Arc<Self>| async move {
                        let options = AcquireRequest::from_dict(&options)?;
                        match &c.reg.write {
                            Some(CharacteristicWrite { method: CharacteristicWriteMethod::Io, .. }) => {
                                let (tx, rx) = oneshot::channel();
                                let req = CharacteristicWriteRequest { mtu: options.mtu, link: options.link, tx };
                                c.reg
                                    .control_handle
                                    .write_request_tx
                                    .send(req)
                                    .await
                                    .map_err(|_| Reject::Failed)?;
                                let fd = rx.await.map_err(|_| Reject::Failed)??;
                                Ok((fd, options.mtu))
                            }
                            _ => Err(Reject::NotSupported),
                        }
                    })
                },
            );
            ib.method_with_cr_async(
                "AcquireNotify",
                ("options",),
                ("fd", "mtu"),
                |ctx, cr, (options,): (PropMap,)| {
                    method_call(ctx, cr, |c: Arc<Self>| async move {
                        let options = AcquireRequest::from_dict(&options)?;
                        match &c.reg.notify {
                            Some(CharacteristicNotify { method: CharacteristicNotifyMethod::Io, .. }) => {
                                let (fd, stream) = make_socket_pair().map_err(|_| Reject::Failed)?;
                                let writer =
                                    CharacteristicWriter { mtu: options.mtu.into(), link: options.link, stream };
                                let _ = c.reg.control_handle.notifier_tx.send(writer).await;
                                Ok((fd, options.mtu))
                            }
                            _ => Err(Reject::NotSupported),
                        }
                    })
                },
            );
        })
    }
}

/// Read value request.
#[derive(Debug, Clone)]
pub struct ReadCharacteristicValueRequest {
    /// Offset.
    pub offset: u16,
    /// Exchanged MTU.
    pub mtu: u16,
    /// Link type.
    pub link: Option<LinkType>,
}

impl ReadCharacteristicValueRequest {
    fn from_dict(dict: &PropMap) -> ReqResult<Self> {
        Ok(Self {
            offset: read_opt_prop!(dict, "offset", u16).unwrap_or_default(),
            mtu: read_prop!(dict, "mtu", u16),
            link: read_opt_prop!(dict, "link", String).and_then(|v| v.parse().ok()),
        })
    }
}

/// Write value request.
#[derive(Debug, Clone)]
pub struct WriteCharacteristicValueRequest {
    /// Start offset.
    pub offset: u16,
    /// Write operation type.
    pub op_type: WriteValueType,
    /// Exchanged MTU.
    pub mtu: u16,
    /// Link type.
    pub link: Option<LinkType>,
    /// True if prepare authorization request.
    pub prepare_authorize: bool,
}

impl WriteCharacteristicValueRequest {
    fn from_dict(dict: &PropMap) -> ReqResult<Self> {
        Ok(Self {
            offset: read_opt_prop!(dict, "offset", u16).unwrap_or_default(),
            op_type: read_opt_prop!(dict, "type", String)
                .map(|s| s.parse().map_err(|_| MethodErr::invalid_arg("type")))
                .transpose()?
                .unwrap_or_default(),
            mtu: read_prop!(dict, "mtu", u16),
            link: read_opt_prop!(dict, "link", String).and_then(|v| v.parse().ok()),
            prepare_authorize: read_opt_prop!(dict, "prepare-authorize", bool).unwrap_or_default(),
        })
    }
}

/// Acquire request.
#[derive(Debug, Clone)]
struct AcquireRequest {
    /// Exchanged MTU.
    pub mtu: u16,
    /// Link type.
    pub link: Option<LinkType>,
}

impl AcquireRequest {
    fn from_dict(dict: &PropMap) -> ReqResult<Self> {
        Ok(Self {
            mtu: read_prop!(dict, "mtu", u16),
            link: read_opt_prop!(dict, "link", String).and_then(|v| v.parse().ok()),
        })
    }
}

// ===========================================================================================
// Characteristic descriptor
// ===========================================================================================

/// Local GATT characteristic descriptor exposed over Bluetooth.
pub struct Descriptor {
    /// 128-bit descriptor UUID.
    pub uuid: Uuid,
    /// Characteristic descriptor flags.
    pub flags: CharacteristicDescriptorFlags,
    /// Read value of characteristic descriptor.
    pub read_value: Option<
        Box<
            dyn Fn(ReadDescriptorValueRequest) -> Pin<Box<dyn Future<Output = ReqResult<Vec<u8>>> + Send>>
                + Send
                + Sync,
        >,
    >,
    /// Write value of characteristic descriptor.
    pub write_value: Option<
        Box<
            dyn Fn(Vec<u8>, WriteDescriptorValueRequest) -> Pin<Box<dyn Future<Output = ReqResult<()>> + Send>>
                + Send
                + Sync,
        >,
    >,
}

impl fmt::Debug for Descriptor {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "CharacteristicDescriptor {{ uuid: {:?}, flags: {:?}, read_value: {:?}, write_value: {:?} }}",
            &self.uuid,
            &self.flags,
            self.read_value.is_some(),
            self.write_value.is_some()
        )
    }
}

impl Descriptor {
    pub(crate) fn register_interface(cr: &mut Crossroads) -> IfaceToken<Arc<Self>> {
        cr.register("org.bluez.GattDescriptor1", |ib: &mut IfaceBuilder<Arc<Self>>| {
            cr_property!(ib, "UUID", cd => {
                Some(cd.uuid.to_string())
            });
            cr_property!(ib, "Flags", cd => {
                Some(cd.flags.to_vec())
            });
            ib.property("Characteristic").get(|ctx, _| {
                let mut comps: Vec<_> = ctx.path().split('/').collect();
                comps.pop();
                let char_path = dbus::Path::new(comps.join("/")).unwrap();
                dbg!(&char_path);
                Ok(char_path)
            });
            ib.method_with_cr_async("ReadValue", ("flags",), ("value",), |ctx, cr, (flags,): (PropMap,)| {
                method_call(ctx, cr, |c: Arc<Self>| async move {
                    dbg!(&flags);
                    let options = ReadDescriptorValueRequest::from_dict(&flags)?;
                    match &c.read_value {
                        Some(read_value) => {
                            let value = read_value(options).await?;
                            Ok((value,))
                        }
                        None => Err(Reject::NotSupported),
                    }
                })
            });
            ib.method_with_cr_async(
                "WriteValue",
                ("value", "flags"),
                (),
                |ctx, cr, (value, flags): (Vec<u8>, PropMap)| {
                    method_call(ctx, cr, |c: Arc<Self>| async move {
                        let options = WriteDescriptorValueRequest::from_dict(&flags)?;
                        match &c.write_value {
                            Some(write_value) => {
                                write_value(value, options).await?;
                                Ok(())
                            }
                            None => Err(Reject::NotSupported),
                        }
                    })
                },
            );
        })
    }
}

/// Read characteristic value request.
#[derive(Debug, Clone)]
pub struct ReadDescriptorValueRequest {
    /// Offset.
    pub offset: u16,
    /// Link type.
    pub link: Option<LinkType>,
}

impl ReadDescriptorValueRequest {
    fn from_dict(dict: &PropMap) -> ReqResult<Self> {
        Ok(Self {
            offset: read_prop!(dict, "offset", u16),
            link: read_opt_prop!(dict, "link", String).and_then(|v| v.parse().ok()),
        })
    }
}

/// Write characteristic value request.
#[derive(Debug, Clone)]
pub struct WriteDescriptorValueRequest {
    /// Offset.
    pub offset: u16,
    /// Link type.
    pub link: Option<LinkType>,
    /// Is prepare authorization request?
    pub prepare_authorize: bool,
}

impl WriteDescriptorValueRequest {
    fn from_dict(dict: &PropMap) -> ReqResult<Self> {
        Ok(Self {
            offset: read_prop!(dict, "offset", u16),
            link: read_opt_prop!(dict, "link", String).and_then(|v| v.parse().ok()),
            prepare_authorize: read_prop!(dict, "prepare_authorize", bool),
        })
    }
}

// ===========================================================================================
// Application
// ===========================================================================================

pub(crate) const GATT_APP_PREFIX: &str = "/io/crates/tokio_bluez/gatt/app/";

/// Local GATT application to publish over Bluetooth.
#[derive(Debug)]
pub struct Application {
    /// Services to publish.
    pub services: Vec<Service>,
}

impl Application {
    pub(crate) async fn register(
        mut self, inner: Arc<SessionInner>, adapter_name: Arc<String>,
    ) -> crate::Result<ApplicationHandle> {
        let mut reg_paths = Vec::new();
        let app_path = format!("{}{}", GATT_APP_PREFIX, Uuid::new_v4().to_simple());
        let app_path = dbus::Path::new(app_path).unwrap();

        {
            let mut cr = inner.crossroads.lock().await;

            let services = take(&mut self.services);
            reg_paths.push(app_path.clone());
            let om = cr.object_manager::<Self>();
            cr.insert(app_path.clone(), &[om], self);

            for (service_idx, mut service) in services.into_iter().enumerate() {
                let chars = take(&mut service.characteristics);
                let service_path = format!("{}/service{}", &app_path, service_idx);
                let service_path = dbus::Path::new(service_path).unwrap();
                reg_paths.push(service_path.clone());
                cr.insert(service_path.clone(), &[inner.gatt_service_token], Arc::new(service));

                for (char_idx, mut char) in chars.into_iter().enumerate() {
                    let descs = take(&mut char.descriptors);

                    let char_path = format!("{}/char{}", &service_path, char_idx);
                    let char_path = dbus::Path::new(char_path).unwrap();
                    reg_paths.push(char_path.clone());
                    cr.insert(char_path.clone(), &[inner.gatt_characteristic_token], Arc::new(char));

                    for (desc_idx, desc) in descs.into_iter().enumerate() {
                        let desc_path = format!("{}/desc{}", &char_path, desc_idx);
                        let desc_path = dbus::Path::new(desc_path).unwrap();
                        reg_paths.push(desc_path.clone());
                        cr.insert(desc_path, &[inner.gatt_characteristic_descriptor_token], Arc::new(desc));
                    }
                }
            }
        }

        let proxy =
            Proxy::new(SERVICE_NAME, Adapter::dbus_path(&*adapter_name)?, TIMEOUT, inner.connection.clone());
        dbg!(&app_path);
        //future::pending::<()>().await;
        proxy.method_call(MANAGER_INTERFACE, "RegisterApplication", (app_path.clone(), PropMap::new())).await?;

        let (drop_tx, drop_rx) = oneshot::channel();
        let app_path_unreg = app_path.clone();
        tokio::spawn(async move {
            let _ = drop_rx.await;
            let _: std::result::Result<(), dbus::Error> =
                proxy.method_call(MANAGER_INTERFACE, "UnregisterApplication", (app_path_unreg,)).await;

            let mut cr = inner.crossroads.lock().await;
            for reg_path in reg_paths {
                let _: Option<Self> = cr.remove(&reg_path);
            }
        });

        Ok(ApplicationHandle { name: app_path, _drop_tx: drop_tx })
    }
}

/// Local GATT application published over Bluetooth.
///
/// Drop this handle to unpublish.
pub struct ApplicationHandle {
    name: dbus::Path<'static>,
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

pub(crate) const GATT_PROFILE_PREFIX: &str = "/io/crates/tokio_bluez/gatt/profile/";

/// Local profile (GATT client) instance.
///
/// By registering this type of object
/// an application effectively indicates support for a specific GATT profile
/// and requests automatic connections to be established to devices
/// supporting it.
#[derive(Debug, Clone)]
pub struct Profile {
    /// 128-bit GATT service UUIDs to auto connect.
    pub uuids: HashSet<Uuid>,
}

impl Profile {
    pub(crate) fn register_interface(cr: &mut Crossroads) -> IfaceToken<Self> {
        cr.register("org.bluez.GattProfile1", |ib: &mut IfaceBuilder<Self>| {
            cr_property!(ib, "UUIDs", p => {
                Some(p.uuids.iter().map(|uuid| uuid.to_string()).collect::<Vec<_>>())
            });
        })
    }

    pub(crate) async fn register(
        self, inner: Arc<SessionInner>, adapter_name: Arc<String>,
    ) -> crate::Result<ProfileHandle> {
        let profile_path = format!("{}{}", GATT_PROFILE_PREFIX, Uuid::new_v4().to_simple());
        let profile_path = dbus::Path::new(profile_path).unwrap();

        {
            let mut cr = inner.crossroads.lock().await;
            let om = cr.object_manager::<Self>();
            cr.insert(profile_path.clone(), &[inner.gatt_profile_token, om], self);
        }

        let proxy =
            Proxy::new(SERVICE_NAME, Adapter::dbus_path(&*adapter_name)?, TIMEOUT, inner.connection.clone());
        dbg!(&profile_path);
        //future::pending::<()>().await;
        proxy
            .method_call(MANAGER_INTERFACE, "RegisterApplication", (profile_path.clone(), PropMap::new()))
            .await?;

        let (drop_tx, drop_rx) = oneshot::channel();
        let profile_path_unreg = profile_path.clone();
        tokio::spawn(async move {
            let _ = drop_rx.await;

            let _: std::result::Result<(), dbus::Error> = proxy
                .method_call(MANAGER_INTERFACE, "UnregisterApplication", (profile_path_unreg.clone(),))
                .await;

            let mut cr = inner.crossroads.lock().await;
            let _: Option<Self> = cr.remove(&profile_path_unreg);
        });

        Ok(ProfileHandle { name: profile_path, _drop_tx: drop_tx })
    }
}

/// Published local profile (GATT client) instance.
///
/// Drop this handle to unpublish.
pub struct ProfileHandle {
    name: dbus::Path<'static>,
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
