use std::{
    fmt,
    marker::PhantomData,
    mem::{swap, take},
    pin::Pin,
    sync::{Arc, Weak},
};

use dbus::{
    arg::{prop_cast, AppendAll, PropMap, RefArg, Variant},
    nonblock::{Proxy, SyncConnection},
    MethodErr, Path,
};
use dbus_crossroads::{Context, Crossroads, IfaceBuilder, IfaceToken};
use futures::{channel::oneshot, future, Future, Stream};
use strum::{Display, EnumString, IntoStaticStr};
use tokio::sync::mpsc;
use uuid::Uuid;

use super::{CharacteristicDescriptorFlags, CharacteristicFlags, WriteValueType};
use crate::{
    all_dbus_objects, Adapter, Address, Device, Error, Result, SessionInner, ERR_PREFIX, SERVICE_NAME, TIMEOUT,
};

pub(crate) const SERVICE_INTERFACE: &str = "org.bluez.GattService1";
pub(crate) const CHARACTERISTIC_INTERFACE: &str = "org.bluez.GattCharacteristic1";
pub(crate) const DESCRIPTOR_INTERFACE: &str = "org.bluez.GattDescriptor1";

// ===========================================================================================
// Service
// ===========================================================================================

/// Interface to remote GATT service connected over Bluetooth.
#[derive(Clone)]
pub struct Service {
    inner: Arc<SessionInner>,
    dbus_path: Path<'static>,
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
            dbus_path: Self::dbus_path(&*adapter_name, device_address, id)?,
            adapter_name,
            device_address,
            id,
        })
    }

    fn proxy(&self) -> Proxy<'_, &SyncConnection> {
        Proxy::new(SERVICE_NAME, &self.dbus_path, TIMEOUT, &*self.inner.connection)
    }

    pub(crate) fn dbus_path(adapter_name: &str, device_address: Address, id: u16) -> Result<Path<'static>> {
        let device_path = Device::dbus_path(adapter_name, device_address)?;
        Ok(Path::new(format!("{}/service{:04x}", device_path, id)).unwrap())
    }

    pub(crate) fn parse_dbus_path_prefix<'a>(path: &'a Path) -> Option<((&'a str, Address, u16), &'a str)> {
        match Device::parse_dbus_path_prefix(path) {
            Some(((adapter_name, device_address), p)) => match p.strip_prefix("service") {
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

    pub(crate) fn parse_dbus_path<'a>(path: &'a Path) -> Option<(&'a str, Address, u16)> {
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
        for (path, interfaces) in all_dbus_objects(&*self.inner.connection).await? {
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

    dbus_interface!();
    dbus_default_interface!(SERVICE_INTERFACE);
}

define_properties!(
    Service, ServiceProperty => {
        /// Indicates whether or not this GATT service is a
        /// primary service.
        ///
        /// If false, the service is secondary.
        property(
            Primary, bool,
            dbus: (SERVICE_INTERFACE, "primary", bool, MANDATORY),
            get: (primary, v => { v.to_owned() }),
        );

        /// 128-bit service UUID.
        property(
            Uuid, Uuid,
            dbus: (SERVICE_INTERFACE, "UUID", String, MANDATORY),
            get: (uuid, v => {v.parse().map_err(|_| Error::InvalidUuid(v.to_string()))?}),
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
    dbus_path: Path<'static>,
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
            dbus_path: Self::dbus_path(&*adapter_name, device_address, id)?,
            adapter_name,
            device_address,
            service_id,
            id,
        })
    }

    fn proxy(&self) -> Proxy<'_, &SyncConnection> {
        Proxy::new(SERVICE_NAME, &self.dbus_path, TIMEOUT, &*self.inner.connection)
    }

    pub(crate) fn dbus_path(adapter_name: &str, device_address: Address, id: u16) -> Result<Path<'static>> {
        let device_path = Device::dbus_path(adapter_name, device_address)?;
        Ok(Path::new(format!("{}/char{:04x}", device_path, id)).unwrap())
    }

    pub(crate) fn parse_dbus_path_prefix<'a>(path: &'a Path) -> Option<((&'a str, Address, u16, u16), &'a str)> {
        match Service::parse_dbus_path_prefix(path) {
            Some(((adapter_name, device_address, service_id), p)) => match p.strip_prefix("char") {
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

    pub(crate) fn parse_dbus_path<'a>(path: &'a Path) -> Option<(&'a str, Address, u16, u16)> {
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
    pub async fn descriptors(&self) -> Result<Vec<CharacteristicDescriptor>> {
        let mut chars = Vec::new();
        for (path, interfaces) in all_dbus_objects(&*self.inner.connection).await? {
            match CharacteristicDescriptor::parse_dbus_path(&path) {
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
    pub async fn descriptor(&self, descriptor_id: u16) -> Result<CharacteristicDescriptor> {
        CharacteristicDescriptor::new(
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
        self.read_ext(&ReadCharacteristicValueRequest::default()).await
    }

    /// Issues a request to read the value of the
    /// characteristic and returns the value if the
    /// operation was successful.    
    ///
    /// Takes extended options for the read operation.
    pub async fn read_ext(&self, req: &ReadCharacteristicValueRequest) -> Result<Vec<u8>> {
        let (value,): (Vec<u8>,) = self.call_method("ReadValue", (req.to_dict(),)).await?;
        Ok(value)
    }

    /// Issues a request to write the value of the characteristic.
    pub async fn write(&self, value: &[u8]) -> Result<()> {
        self.write_ext(value, &WriteCharacteristicValueRequest::default()).await
    }

    /// Issues a request to write the value of the characteristic.
    ///
    /// Takes extended options for the write operation.
    pub async fn write_ext(&self, value: &[u8], req: &WriteCharacteristicValueRequest) -> Result<()> {
        let () = self.call_method("WriteValue", (value, req.to_dict())).await?;
        Ok(())
    }

    /// Starts a notification session from this characteristic
    /// if it supports value notifications or indications.    
    pub async fn notify(&self) -> Result<impl Stream<Item = Vec<u8>>> {
        // So how do we implement this?
        // The problem here is as before that we might have to share a notify session.
        // Let's see how we handled this before.
        // Problem here is race condition during session termination.
        // So it would need two slots,
        // one for send one for receive.
        todo!()
    }

    async fn notify_session(&self) -> Result<Arc<oneshot::Sender<()>>> {
        let mut notify_slots = self.inner.notify_slots.lock().await;
        if let Some((term_tx_weak, termed_rx)) = notify_slots.get_mut(&self.dbus_path) {
            match term_tx_weak.upgrade() {
                Some(term_tx) => return Ok(term_tx),
                None => {
                    let _ = termed_rx.await;
                }
            }
        }

        let (term_tx, term_rx) = oneshot::channel();
        let (termed_tx, termed_rx) = oneshot::channel();

        let term_tx = Arc::new(term_tx);
        notify_slots.insert(self.dbus_path.clone(), (Arc::downgrade(&term_tx), termed_rx));

        tokio::spawn(async move {});

        Ok(term_tx)
    }

    dbus_interface!();
    dbus_default_interface!(CHARACTERISTIC_INTERFACE);
}

/// Read value request.
#[derive(Debug, Default, Clone)]
pub struct ReadCharacteristicValueRequest {
    /// Offset.
    pub offset: u16,
}

impl ReadCharacteristicValueRequest {
    fn to_dict(&self) -> PropMap {
        let mut pm = PropMap::new();
        pm.insert("offset".to_string(), Variant(self.offset.box_clone()));
        pm
    }
}

/// Write value request.
#[derive(Debug, Default, Clone)]
pub struct WriteCharacteristicValueRequest {
    /// Start offset.
    pub offset: u16,
    /// Write operation type.
    pub op_type: WriteValueType,
    /// True if prepare authorization request.
    pub prepare_authorize: bool,
}

impl WriteCharacteristicValueRequest {
    fn to_dict(&self) -> PropMap {
        let mut pm = PropMap::new();
        pm.insert("offset".to_string(), Variant(self.offset.box_clone()));
        pm.insert("type".to_string(), Variant(self.op_type.to_string().box_clone()));
        pm.insert("prepare-authorize".to_string(), Variant(self.prepare_authorize.box_clone()));
        pm
    }
}

define_properties!(
    Characteristic, CharacteristicProperty => {
        /// 128-bit characteristic UUID.
        property(
            Uuid, Uuid,
            dbus: (CHARACTERISTIC_INTERFACE, "UUID", String, MANDATORY),
            get: (uuid, v => {v.parse().map_err(|_| Error::InvalidUuid(v.to_string()))?}),
        );

        /// True, if this characteristic has been acquired by any
        /// client using AcquireWrite.
        ///
        /// It is ommited in case the 'write-without-response' flag is not set.
        property(
            WriteAcquired, bool,
            dbus: (CHARACTERISTIC_INTERFACE, "WriteAcquired", bool, OPTIONAL),
            get: (write_acquired, v => {v.to_owned()}),
        );

        /// True, if this characteristic has been acquired by any
        /// client using AcquireNotify.
        ///
        /// It is ommited in case the 'notify' flag is not set.
        property(
            NotifyAcquired, bool,
            dbus: (CHARACTERISTIC_INTERFACE, "NotifyAcquired", bool, OPTIONAL),
            get: (notify_acquired, v => {v.to_owned()}),
        );

        ///	True, if notifications or indications on this
        ///	characteristic are currently enabled.
        property(
            Notifying, bool,
            dbus: (CHARACTERISTIC_INTERFACE, "Notifying", bool, MANDATORY),
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
            get: (flags, v => {CharacteristicFlags::from_slice(v)}),
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
    }
);

// ===========================================================================================
// Characteristic descriptor
// ===========================================================================================

/// Interface to remote GATT characteristic descriptor connected over Bluetooth.
#[derive(Clone)]
pub struct CharacteristicDescriptor {
    inner: Arc<SessionInner>,
    dbus_path: Path<'static>,
    adapter_name: Arc<String>,
    device_address: Address,
    service_id: u16,
    characteristic_id: u16,
    id: u16,
}

impl fmt::Debug for CharacteristicDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter) -> std::fmt::Result {
        write!(f, "CharacteristicDescriptor {{ adapter_name: {}, device_address: {}, service_id: {}, characteristic_id: {}, id: {} }}", 
            self.adapter_name(), self.device_address(), self.service_id(), self.characteristic_id(), self.id())
    }
}

impl CharacteristicDescriptor {
    pub(crate) fn new(
        inner: Arc<SessionInner>, adapter_name: Arc<String>, device_address: Address, service_id: u16,
        characteristic_id: u16, id: u16,
    ) -> Result<Self> {
        Ok(Self {
            inner,
            dbus_path: Self::dbus_path(&*adapter_name, device_address, id)?,
            adapter_name,
            device_address,
            service_id,
            characteristic_id,
            id,
        })
    }

    fn proxy(&self) -> Proxy<'_, &SyncConnection> {
        Proxy::new(SERVICE_NAME, &self.dbus_path, TIMEOUT, &*self.inner.connection)
    }

    pub(crate) fn dbus_path(adapter_name: &str, device_address: Address, id: u16) -> Result<Path<'static>> {
        let device_path = Device::dbus_path(adapter_name, device_address)?;
        Ok(Path::new(format!("{}/desc{:04x}", device_path, id)).unwrap())
    }

    pub(crate) fn parse_dbus_path_prefix<'a>(
        path: &'a Path,
    ) -> Option<((&'a str, Address, u16, u16, u16), &'a str)> {
        match Characteristic::parse_dbus_path_prefix(path) {
            Some(((adapter_name, device_address, service_id, char_id), p)) => match p.strip_prefix("desc") {
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

    pub(crate) fn parse_dbus_path<'a>(path: &'a Path) -> Option<(&'a str, Address, u16, u16, u16)> {
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

    dbus_interface!();
    dbus_default_interface!(DESCRIPTOR_INTERFACE);
}

define_properties!(
    CharacteristicDescriptor, CharacteristicDescriptorProperty => {
        /// 128-bit descriptor UUID.
        property(
            Uuid, Uuid,
            dbus: (DESCRIPTOR_INTERFACE, "UUID", String, MANDATORY),
            get: (uuid, v => {v.parse().map_err(|_| Error::InvalidUuid(v.to_string()))?}),
        );

        /// Defines how the descriptor value can be used.
        property(
            Flags, CharacteristicDescriptorFlags,
            dbus: (DESCRIPTOR_INTERFACE, "Flags", Vec<String>, MANDATORY),
            get: (flags, v => {CharacteristicDescriptorFlags::from_slice(v)}),
        );

        /// The cached value of the descriptor.
        ///
        /// This property
        /// gets updated only after a successful read request and
        /// when a notification or indication is received.
        property(
            CachedValue, Vec<u8>,
            dbus: (DESCRIPTOR_INTERFACE, "Value", Vec<u8>, MANDATORY),
            get: (cached_value, v => {v.to_owned()}),
        );
    }
);
