//! Consume remote GATT services of connected devices.

use futures::{Stream, StreamExt};
use std::{
    collections::HashMap,
    fmt,
    os::unix::prelude::{FromRawFd, IntoRawFd},
    sync::Arc,
};
use tokio::net::UnixDatagram;
use uuid::Uuid;
use zbus::{
    zvariant::{OwnedFd, OwnedObjectPath, OwnedValue},
    Proxy,
};

use super::{
    mtu_workaround, CharacteristicFlags, CharacteristicReader, CharacteristicWriter, WriteOp,
    CHARACTERISTIC_INTERFACE, DESCRIPTOR_INTERFACE, SERVICE_INTERFACE,
};
use crate::{
    all_dbus_objects, Address, Device, Error, ErrorKind, Event, InternalErrorKind, Result, SessionInner,
    SingleSessionToken, SERVICE_NAME,
};

// ===========================================================================================
// Service
// ===========================================================================================

/// Interface to remote GATT service connected over Bluetooth.
#[derive(Clone)]
pub struct Service {
    inner: Arc<SessionInner>,
    dbus_path: OwnedObjectPath,
    adapter_name: Arc<String>,
    device_address: Address,
    id: u16,
}

impl fmt::Debug for Service {
    fn fmt(&self, f: &mut fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "Service {{ adapter_name: {}, device_address: {}, id: {} }}",
            self.adapter_name(),
            self.device_address(),
            self.id()
        )
    }
}

impl Service {
    pub(crate) fn new(
        inner: Arc<SessionInner>, adapter_name: Arc<String>, device_address: Address, id: u16,
    ) -> Result<Self> {
        Ok(Self {
            inner,
            dbus_path: Self::dbus_path(&adapter_name, device_address, id)?,
            adapter_name,
            device_address,
            id,
        })
    }

    pub(crate) fn dbus_path(adapter_name: &str, device_address: Address, id: u16) -> Result<OwnedObjectPath> {
        let device_path = Device::dbus_path(adapter_name, device_address)?;
        Ok(OwnedObjectPath::try_from(format!("{device_path}/service{id:04x}")).unwrap())
    }

    pub(crate) fn parse_dbus_path_prefix<'a>(
        path: &'a OwnedObjectPath,
    ) -> Option<((&'a str, Address, u16), &'a str)> {
        match Device::parse_dbus_path_prefix(path) {
            Some(((adapter_name, device_address), p)) => match p.strip_prefix("/service") {
                Some(p) => {
                    let sep = p.find('/').unwrap_or(p.len());
                    match u16::from_str_radix(&p[0..sep], 16) {
                        Ok(id) => Some(((adapter_name, device_address, id), &p[sep..])),
                        Err(_) => None,
                    }
                }
                None => None,
            },
            None => None,
        }
    }

    pub(crate) fn parse_dbus_path<'a>(path: &'a OwnedObjectPath) -> Option<(&'a str, Address, u16)> {
        match Self::parse_dbus_path_prefix(path) {
            Some((v, "")) => Some(v),
            _ => None,
        }
    }

    /// The Bluetooth adapter name.
    pub fn adapter_name(&self) -> &str {
        &self.adapter_name
    }

    /// The Bluetooth device address of the remote device this service belongs to.
    pub fn device_address(&self) -> Address {
        self.device_address
    }

    /// The local identifier for this service.
    ///
    /// It may change when the device is next discovered and is not related to the service UUID.
    pub fn id(&self) -> u16 {
        self.id
    }

    /// GATT characteristics belonging to this service.
    pub async fn characteristics(&self) -> Result<Vec<Characteristic>> {
        let mut chars = Vec::new();
        for (path, interfaces) in all_dbus_objects(&self.inner.connection).await? {
            match Characteristic::parse_dbus_path(&path) {
                Some((adapter, device_address, service_id, id))
                    if adapter == *self.adapter_name
                        && device_address == self.device_address
                        && service_id == self.id
                        && interfaces.contains_key(CHARACTERISTIC_INTERFACE) =>
                {
                    chars.push(self.characteristic(id).await?)
                }
                _ => (),
            }
        }
        Ok(chars)
    }

    /// GATT characteristics with specified id.
    pub async fn characteristic(&self, characteristic_id: u16) -> Result<Characteristic> {
        Characteristic::new(
            self.inner.clone(),
            self.adapter_name.clone(),
            self.device_address,
            self.id,
            characteristic_id,
        )
    }

    zbus_interface!(SERVICE_INTERFACE);
}

define_properties!(
    Service,
    /// GATT service property.
    pub ServiceProperty => {
        /// Indicates whether or not this GATT service is a
        /// primary service.
        ///
        /// If false, the service is secondary.
        property(
            Primary, bool,
            dbus: (SERVICE_INTERFACE, "Primary", bool, MANDATORY),
            get: (primary, v => { v.to_owned() }),
        );

        /// 128-bit service UUID.
        property(
            Uuid, Uuid,
            dbus: (SERVICE_INTERFACE, "UUID", String, MANDATORY),
            get: (uuid, v => {v.parse().map_err(|_| Error::new(ErrorKind::Internal(InternalErrorKind::InvalidUuid(v.to_string()))))?}),
        );

        /// Service ids of included services of this service.
        property(
            Includes, Vec<u16>,
            dbus: (SERVICE_INTERFACE, "Includes", Vec<OwnedObjectPath>, MANDATORY),
            get: (includes, v => {
                v.iter().filter_map(|path| Service::parse_dbus_path(path).map(|(_, _, service_id)| service_id)).collect()
            }),
        );
    }
);

// ===========================================================================================
// Characteristic
// ===========================================================================================

/// Interface to remote GATT characteristic connected over Bluetooth.
#[derive(Clone)]
pub struct Characteristic {
    inner: Arc<SessionInner>,
    dbus_path: OwnedObjectPath,
    adapter_name: Arc<String>,
    device_address: Address,
    service_id: u16,
    id: u16,
}

impl fmt::Debug for Characteristic {
    fn fmt(&self, f: &mut fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "Characteristic {{ adapter_name: {}, device_address: {}, service_id: {}, id: {} }}",
            self.adapter_name(),
            self.device_address(),
            self.service_id(),
            self.id()
        )
    }
}

impl Characteristic {
    pub(crate) fn new(
        inner: Arc<SessionInner>, adapter_name: Arc<String>, device_address: Address, service_id: u16, id: u16,
    ) -> Result<Self> {
        Ok(Self {
            inner,
            dbus_path: Self::dbus_path(&adapter_name, device_address, service_id, id)?,
            adapter_name,
            device_address,
            service_id,
            id,
        })
    }

    async fn proxy(&self) -> Result<Proxy<'_>> {
        Ok(Proxy::new(&self.inner.connection, SERVICE_NAME, &self.dbus_path, CHARACTERISTIC_INTERFACE).await?)
    }

    pub(crate) fn dbus_path(
        adapter_name: &str, device_address: Address, service_id: u16, id: u16,
    ) -> Result<OwnedObjectPath> {
        let service_path = Service::dbus_path(adapter_name, device_address, service_id)?;
        Ok(OwnedObjectPath::try_from(format!("{service_path}/char{id:04x}")).unwrap())
    }

    #[allow(clippy::type_complexity)]
    pub(crate) fn parse_dbus_path_prefix<'a>(
        path: &'a OwnedObjectPath,
    ) -> Option<((&'a str, Address, u16, u16), &'a str)> {
        match Service::parse_dbus_path_prefix(path) {
            Some(((adapter_name, device_address, service_id), p)) => match p.strip_prefix("/char") {
                Some(p) => {
                    let sep = p.find('/').unwrap_or(p.len());
                    match u16::from_str_radix(&p[0..sep], 16) {
                        Ok(id) => Some(((adapter_name, device_address, service_id, id), &p[sep..])),
                        Err(_) => None,
                    }
                }
                None => None,
            },
            None => None,
        }
    }

    pub(crate) fn parse_dbus_path<'a>(path: &'a OwnedObjectPath) -> Option<(&'a str, Address, u16, u16)> {
        match Self::parse_dbus_path_prefix(path) {
            Some((v, "")) => Some(v),
            _ => None,
        }
    }

    /// The Bluetooth adapter name.
    pub fn adapter_name(&self) -> &str {
        &self.adapter_name
    }

    /// The Bluetooth device address of the remote device this service belongs to.
    pub fn device_address(&self) -> Address {
        self.device_address
    }

    /// The local identifier for the service this characteristic belongs to.
    pub fn service_id(&self) -> u16 {
        self.service_id
    }

    /// The local identifier for this characteristic.
    ///
    /// It may change when the device is next discovered and is not related to the characteristic UUID.
    pub fn id(&self) -> u16 {
        self.id
    }

    /// GATT descriptors belonging to this characteristic.
    pub async fn descriptors(&self) -> Result<Vec<Descriptor>> {
        let mut chars = Vec::new();
        for (path, interfaces) in all_dbus_objects(&self.inner.connection).await? {
            match Descriptor::parse_dbus_path(&path) {
                Some((adapter, device_address, service_id, char_id, id))
                    if adapter == *self.adapter_name
                        && device_address == self.device_address
                        && service_id == self.service_id
                        && char_id == self.id
                        && interfaces.contains_key(DESCRIPTOR_INTERFACE) =>
                {
                    chars.push(self.descriptor(id).await?)
                }
                _ => (),
            }
        }
        Ok(chars)
    }

    /// GATT descriptor with specified id.
    pub async fn descriptor(&self, descriptor_id: u16) -> Result<Descriptor> {
        Descriptor::new(
            self.inner.clone(),
            self.adapter_name.clone(),
            self.device_address,
            self.service_id,
            self.id,
            descriptor_id,
        )
    }

    /// Issues a request to read the value of the
    /// characteristic and returns the value if the
    /// operation was successful.
    pub async fn read(&self) -> Result<Vec<u8>> {
        self.read_ext(&CharacteristicReadRequest::default()).await
    }

    /// Issues a request to read the value of the
    /// characteristic and returns the value if the
    /// operation was successful.
    ///
    /// Takes extended options for the read operation.
    pub async fn read_ext(&self, req: &CharacteristicReadRequest) -> Result<Vec<u8>> {
        let proxy = self.proxy().await?;
        let (value,): (Vec<u8>,) = proxy.call("ReadValue", &(req.to_dict(),)).await?;
        Ok(value)
    }

    /// Issues a request to write the value of the characteristic.
    pub async fn write(&self, value: &[u8]) -> Result<()> {
        self.write_ext(value, &CharacteristicWriteRequest::default()).await
    }

    /// Issues a request to write the value of the characteristic.
    ///
    /// Takes extended options for the write operation.
    pub async fn write_ext(&self, value: &[u8], req: &CharacteristicWriteRequest) -> Result<()> {
        let proxy = self.proxy().await?;
        let () = proxy.call("WriteValue", &(value, req.to_dict())).await?;
        Ok(())
    }

    /// Acquire writer for writing with low overhead.
    ///
    /// It only works with characteristic that has
    /// the [write_without_response](CharacteristicFlags::write_without_response) flag set.
    ///
    /// Usage of [write](Self::write) will be
    /// locked causing it to return NotPermitted error.
    /// To release the lock the client shall drop the writer.
    ///
    /// Note: the MTU can only be negotiated once and is
    /// symmetric therefore this method may be delayed in
    /// order to have the exchange MTU completed, because of
    /// that the file descriptor is closed during
    /// reconnections as the MTU has to be renegotiated.
    pub async fn write_io(&self) -> Result<CharacteristicWriter> {
        let options: HashMap<String, OwnedValue> = HashMap::new();
        let proxy = self.proxy().await?;
        let (fd, mtu): (OwnedFd, u16) = proxy.call("AcquireWrite", &(options,)).await?;
        let fd: std::os::unix::io::OwnedFd = fd.into();
        let socket = unsafe { std::os::unix::net::UnixDatagram::from_raw_fd(fd.into_raw_fd()) };
        socket.set_nonblocking(true)?;
        let socket = UnixDatagram::from_std(socket)?;
        let mtu = mtu_workaround(mtu.into());
        Ok(CharacteristicWriter {
            adapter_name: self.adapter_name().to_string(),
            device_address: self.device_address,
            mtu,
            socket,
        })
    }

    /// Starts a notification or indication session from this characteristic
    /// if it supports value notifications or indications.
    ///
    /// This will also notify after a read operation.
    pub async fn notify(&self) -> Result<impl Stream<Item = Vec<u8>>> {
        let token = self.notify_session().await?;
        let events = self.inner.events(self.dbus_path.clone(), false).await?;
        let values = events.filter_map(move |evt| {
            let _token = &token;
            async move {
                if let Event::PropertiesChanged { changed, .. } = evt {
                    for property in CharacteristicProperty::from_prop_map(&changed) {
                        if let CharacteristicProperty::CachedValue(value) = property {
                            return Some(value);
                        }
                    }
                }
                None
            }
        });
        Ok(values)
    }

    async fn notify_session(&self) -> Result<SingleSessionToken> {
        let dbus_path = self.dbus_path.clone();
        let connection = self.inner.connection.clone();
        let dbus_path2 = dbus_path.clone();
        let connection2 = connection.clone();
        self.inner
            .single_session(
                &self.dbus_path,
                async move {
                    let proxy =
                        Proxy::new(&connection, SERVICE_NAME, &dbus_path, CHARACTERISTIC_INTERFACE).await?;
                    let () = proxy.call("StartNotify", &()).await?;
                    Ok(())
                },
                async move {
                    log::trace!("{}: {}.StopNotify ()", &dbus_path2, SERVICE_NAME);
                    let proxy =
                        Proxy::new(&connection2, SERVICE_NAME, &dbus_path2, CHARACTERISTIC_INTERFACE).await;
                    if let Ok(proxy) = proxy {
                        let result: zbus::Result<()> = proxy.call("StopNotify", &()).await;
                        log::trace!("{}: {}.StopNotify () -> {:?}", &dbus_path2, SERVICE_NAME, &result);
                    }
                },
            )
            .await
    }

    /// Acquire reader for notify with low overhead.
    ///
    /// It only works with characteristic that has
    /// the [notify](CharacteristicFlags::notify) flag set and no other client has called
    /// [notify](Self::notify) or [notify_io](Self::notify_io).
    ///
    /// Notification are enabled during this procedure so
    /// [notify](Self::notify) shall not be called, any notification
    /// will be dispatched via file descriptor therefore the
    /// Value property is not affected during the time where
    /// notify has been acquired.
    ///
    /// Usage of [notify](Self::notify) will be
    /// locked causing it to return NotPermitted error.
    /// To release the lock the client shall drop the writer.
    ///
    /// Note: the MTU can only be negotiated once and is
    /// symmetric therefore this method may be delayed in
    /// order to have the exchange MTU completed, because of
    /// that the file descriptor is closed during
    /// reconnections as the MTU has to be renegotiated.
    pub async fn notify_io(&self) -> Result<CharacteristicReader> {
        let options: HashMap<String, OwnedValue> = HashMap::new();
        let proxy = self.proxy().await?;
        let (fd, mtu): (OwnedFd, u16) = proxy.call("AcquireNotify", &(options,)).await?;
        let fd: std::os::unix::io::OwnedFd = fd.into();
        let socket = unsafe { std::os::unix::net::UnixDatagram::from_raw_fd(fd.into_raw_fd()) };
        socket.set_nonblocking(true)?;
        let socket = UnixDatagram::from_std(socket)?;
        Ok(CharacteristicReader {
            adapter_name: self.adapter_name().to_string(),
            device_address: self.device_address,
            mtu: mtu.into(),
            socket,
            buf: Vec::new(),
        })
    }

    zbus_interface!(CHARACTERISTIC_INTERFACE);
}

/// Read characteristic value extended request.
#[derive(Debug, Default, Clone)]
pub struct CharacteristicReadRequest {
    /// Offset.
    pub offset: u16,
    #[doc(hidden)]
    pub _non_exhaustive: (),
}

impl CharacteristicReadRequest {
    fn to_dict(&self) -> HashMap<String, OwnedValue> {
        let mut pm = HashMap::new();
        pm.insert("offset".to_string(), OwnedValue::from(self.offset));
        pm
    }
}

/// Write characteristic value extended request.
#[derive(Debug, Default, Clone)]
pub struct CharacteristicWriteRequest {
    /// Start offset.
    pub offset: u16,
    /// Write operation type.
    pub op_type: WriteOp,
    /// True if prepare authorization request.
    pub prepare_authorize: bool,
    #[doc(hidden)]
    pub _non_exhaustive: (),
}

impl CharacteristicWriteRequest {
    fn to_dict(&self) -> HashMap<String, OwnedValue> {
        let mut pm = HashMap::new();
        pm.insert("offset".to_string(), OwnedValue::from(self.offset));
        pm.insert("type".to_string(), OwnedValue::from(zbus::zvariant::Str::from(self.op_type.to_string())));
        pm.insert("prepare-authorize".to_string(), OwnedValue::from(self.prepare_authorize));
        pm
    }
}

define_properties!(
    Characteristic,
    /// GATT characteristic property.
    pub CharacteristicProperty => {
        /// 128-bit characteristic UUID.
        property(
            Uuid, Uuid,
            dbus: (CHARACTERISTIC_INTERFACE, "UUID", String, MANDATORY),
            get: (uuid, v => {v.parse().map_err(|_| Error::new(ErrorKind::Internal(InternalErrorKind::InvalidUuid(v.to_string()))))?}),
        );

        ///	Whether notifications or indications on this
        ///	characteristic are supported and currently enabled.
        ///
        /// Returns `Some(true)` if enabled and `Some(false)` if disabled.
        /// When notifications are unsupported this returns `None`.
        property(
            Notifying, bool,
            dbus: (CHARACTERISTIC_INTERFACE, "Notifying", bool, OPTIONAL),
            get: (notifying, v => {v.to_owned()}),
        );

        /// Defines how the characteristic value can be used.
        ///
        /// See
        /// Core spec "Table 3.5: Characteristic Properties bit
        /// field", and "Table 3.8: Characteristic Extended
        /// Properties bit field".
        property(
            Flags, CharacteristicFlags,
            dbus: (CHARACTERISTIC_INTERFACE, "Flags", Vec<String>, MANDATORY),
            get: (flags, v => {CharacteristicFlags::from_slice(&v)}),
        );

        /// The cached value of the characteristic.
        ///
        /// This property
        /// gets updated only after a successful read request and
        /// when a notification or indication is received.
        property(
            CachedValue, Vec<u8>,
            dbus: (CHARACTERISTIC_INTERFACE, "Value", Vec<u8>, MANDATORY),
            get: (cached_value, v => {v.to_owned()}),
        );

        /// The maximum transmission unit for the characteristic.
        ///
        /// This is the maximum amount of data that can be sent or received
        /// in a single packet for this characteristic. Longer data may be
        /// able to be sent or received using long procedures when available.
        property(
            Mtu, usize,
            dbus: (CHARACTERISTIC_INTERFACE, "MTU", u16, MANDATORY),
            get: (mtu, v => { mtu_workaround(v.into()) }),
        );
    }
);

// ===========================================================================================
// Characteristic descriptor
// ===========================================================================================

/// Interface to remote GATT characteristic descriptor connected over Bluetooth.
#[derive(Clone)]
pub struct Descriptor {
    inner: Arc<SessionInner>,
    dbus_path: OwnedObjectPath,
    adapter_name: Arc<String>,
    device_address: Address,
    service_id: u16,
    characteristic_id: u16,
    id: u16,
}

impl fmt::Debug for Descriptor {
    fn fmt(&self, f: &mut fmt::Formatter) -> std::fmt::Result {
        write!(f, "Descriptor {{ adapter_name: {}, device_address: {}, service_id: {}, characteristic_id: {}, id: {} }}",
            self.adapter_name(), self.device_address(), self.service_id(), self.characteristic_id(), self.id())
    }
}

impl Descriptor {
    pub(crate) fn new(
        inner: Arc<SessionInner>, adapter_name: Arc<String>, device_address: Address, service_id: u16,
        characteristic_id: u16, id: u16,
    ) -> Result<Self> {
        Ok(Self {
            inner,
            dbus_path: Self::dbus_path(&adapter_name, device_address, service_id, characteristic_id, id)?,
            adapter_name,
            device_address,
            service_id,
            characteristic_id,
            id,
        })
    }

    async fn proxy(&self) -> Result<Proxy<'_>> {
        Ok(Proxy::new(&self.inner.connection, SERVICE_NAME, &self.dbus_path, DESCRIPTOR_INTERFACE).await?)
    }

    pub(crate) fn dbus_path(
        adapter_name: &str, device_address: Address, service_id: u16, characteristic_id: u16, id: u16,
    ) -> Result<OwnedObjectPath> {
        let char_path = Characteristic::dbus_path(adapter_name, device_address, service_id, characteristic_id)?;
        Ok(OwnedObjectPath::try_from(format!("{char_path}/desc{id:04x}")).unwrap())
    }

    #[allow(clippy::type_complexity)]
    pub(crate) fn parse_dbus_path_prefix<'a>(
        path: &'a OwnedObjectPath,
    ) -> Option<((&'a str, Address, u16, u16, u16), &'a str)> {
        match Characteristic::parse_dbus_path_prefix(path) {
            Some(((adapter_name, device_address, service_id, char_id), p)) => match p.strip_prefix("/desc") {
                Some(p) => {
                    let sep = p.find('/').unwrap_or(p.len());
                    match u16::from_str_radix(&p[0..sep], 16) {
                        Ok(id) => Some(((adapter_name, device_address, service_id, char_id, id), &p[sep..])),
                        Err(_) => None,
                    }
                }
                None => None,
            },
            None => None,
        }
    }

    pub(crate) fn parse_dbus_path<'a>(path: &'a OwnedObjectPath) -> Option<(&'a str, Address, u16, u16, u16)> {
        match Self::parse_dbus_path_prefix(path) {
            Some((v, "")) => Some(v),
            _ => None,
        }
    }

    /// The Bluetooth adapter name.
    pub fn adapter_name(&self) -> &str {
        &self.adapter_name
    }

    /// The Bluetooth device address of the remote device this service belongs to.
    pub fn device_address(&self) -> Address {
        self.device_address
    }

    /// The local identifier for the service this characteristic belongs to.
    pub fn service_id(&self) -> u16 {
        self.service_id
    }

    /// The local identifier for the characteristic this descriptor belongs to.
    pub fn characteristic_id(&self) -> u16 {
        self.characteristic_id
    }

    /// The local identifier for this characteristic descriptor.
    ///
    /// It may change when the device is next discovered and is not related to the characteristic descriptor UUID.
    pub fn id(&self) -> u16 {
        self.id
    }

    zbus_interface!(DESCRIPTOR_INTERFACE);

    /// Issues a request to read the value of the
    /// descriptor and returns the value if the
    /// operation was successful.
    pub async fn read(&self) -> Result<Vec<u8>> {
        self.read_ext(&DescriptorReadRequest::default()).await
    }

    /// Issues a request to read the value of the
    /// descriptor and returns the value if the
    /// operation was successful.
    ///
    /// Takes extended options for the read operation.
    pub async fn read_ext(&self, req: &DescriptorReadRequest) -> Result<Vec<u8>> {
        let proxy = self.proxy().await?;
        let (value,): (Vec<u8>,) = proxy.call("ReadValue", &(req.to_dict(),)).await?;
        Ok(value)
    }

    /// Issues a request to write the value of the descriptor.
    pub async fn write(&self, value: &[u8]) -> Result<()> {
        self.write_ext(value, &DescriptorWriteRequest::default()).await
    }

    /// Issues a request to write the value of the descriptor.
    ///
    /// Takes extended options for the write operation.
    pub async fn write_ext(&self, value: &[u8], req: &DescriptorWriteRequest) -> Result<()> {
        let proxy = self.proxy().await?;
        let () = proxy.call("WriteValue", &(value, req.to_dict())).await?;
        Ok(())
    }
}

/// Read characteristic descriptor value extended request.
#[derive(Debug, Default, Clone)]
pub struct DescriptorReadRequest {
    /// Offset.
    pub offset: u16,
    #[doc(hidden)]
    pub _non_exhaustive: (),
}

impl DescriptorReadRequest {
    fn to_dict(&self) -> HashMap<String, OwnedValue> {
        let mut pm = HashMap::new();
        pm.insert("offset".to_string(), OwnedValue::from(self.offset));
        pm
    }
}

/// Write characteristic descriptor value extended request.
#[derive(Debug, Default, Clone)]
pub struct DescriptorWriteRequest {
    /// Start offset.
    pub offset: u16,
    /// True if prepare authorization request.
    pub prepare_authorize: bool,
    #[doc(hidden)]
    pub _non_exhaustive: (),
}

impl DescriptorWriteRequest {
    fn to_dict(&self) -> HashMap<String, OwnedValue> {
        let mut pm = HashMap::new();
        pm.insert("offset".to_string(), OwnedValue::from(self.offset));
        pm.insert("prepare-authorize".to_string(), OwnedValue::from(self.prepare_authorize));
        pm
    }
}

define_properties!(
    Descriptor,
    /// GATT characteristic descriptor property.
    pub CharacteristicDescriptorProperty => {
        /// 128-bit descriptor UUID.
        property(
            Uuid, Uuid,
            dbus: (DESCRIPTOR_INTERFACE, "UUID", String, MANDATORY),
            get: (uuid, v => {v.parse().map_err(|_| Error::new(ErrorKind::Internal(InternalErrorKind::InvalidUuid(v.to_string()))))?}),
        );

        // /// Defines how the descriptor value can be used.
        // property(
        //     Flags, DescriptorFlags,
        //     dbus: (DESCRIPTOR_INTERFACE, "Flags", Vec<String>, MANDATORY),
        //     get: (flags, v => {DescriptorFlags::from_slice(v)}),
        // );

        /// The cached value of the descriptor.
        ///
        /// This property gets updated only after a successful read request.
        property(
            CachedValue, Vec<u8>,
            dbus: (DESCRIPTOR_INTERFACE, "Value", Vec<u8>, MANDATORY),
            get: (cached_value, v => {v.to_owned()}),
        );
    }
);
