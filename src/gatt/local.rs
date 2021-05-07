use strum::{Display, IntoStaticStr};
use thiserror::Error;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::ERR_PREFIX;

// How to define that?
// Fixed hierarchy or not?
// Adding is not possible anyway.
// So define it service-by-service.
// Or group them as application? => Yes, probably better to handle
//
// Service contains characteristics
//    and some properties
//    but cannot be read/written
//
// Characteristic
//    has properties (uuid, state, flags)
//    and methods (read and write value, acquire read and write to get fd?,
//                 acquire notify fd?)
//
// Characteristic descriptior
//    has properties (uuid, flags)
//    and read and write methods (no notifications)
//

// So let user pass an application hierarchy.

/// Local GATT application published over Bluetooth.
pub struct Application {
    pub services: Vec<Service>,
}

/// Local GATT service exposed over Bluetooth.
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

/// Read value request.
pub struct ReadValueRequest {
    /// Offset.
    pub offset: u16,
    /// Exchanged MTU.
    pub mtu: u16,
}

/// Read value operation error.
#[derive(Clone, Debug, Error, IntoStaticStr)]
pub enum ReadValueError {
    #[error("Bluetooth operation failed")]
    Failed,
    #[error("Bluetooth operation in progress")]
    InProgress,
    #[error("Bluetooth operation not permitted")]
    NotPermitted,
    #[error("Bluetooth operation not authorized")]
    NotAuthorized,
    #[error("Invalid offset for Bluetooth GATT property")]
    InvalidOffset,
    #[error("Bluetooth operation not supported")]
    NotSupported,
}

impl From<ReadValueError> for dbus::Error {
    fn from(err: ReadValueError) -> Self {
        let name: &'static str = err.clone().into();
        Self::new_custom(ERR_PREFIX.to_string() + name, &err.to_string())
    }
}

/// Write value request.
pub struct WriteValueRequest {
    /// Start offset.
    pub offset: u16,
    /// Write operation type.
    pub op_type: WriteValueType,
    /// Exchanged MTU.
    pub mtu: u16,
    /// Link type.
    pub link: String, // TODO
    /// True if prepare authorization request.
    pub prepare_authorize: bool,
}

/// Write operation type.
pub enum WriteValueType {
    /// Write without response.
    Command,
    /// Write with response.
    Request,
    /// Reliable Write.
    Reliable,
}

/// Write value operation error.
#[derive(Clone, Debug, Error, IntoStaticStr)]
pub enum WriteValueError {
    #[error("Bluetooth operation failed")]
    Failed,
    #[error("Bluetooth operation in progress")]
    InProgress,
    #[error("Bluetooth operation not permitted")]
    NotPermitted,
    #[error("Invalid value length for Bluetooth GATT property")]
    InvalidValueLength,
    #[error("Bluetooth operation not authorized")]
    NotAuthorized,
    #[error("Bluetooth operation not supported")]
    NotSupported,
}

impl From<WriteValueError> for dbus::Error {
    fn from(err: WriteValueError) -> Self {
        let name: &'static str = err.clone().into();
        Self::new_custom(ERR_PREFIX.to_string() + name, &err.to_string())
    }
}

/// Notify operation error.
#[derive(Clone, Debug, Error, IntoStaticStr)]
pub enum NotifyError {
    #[error("Bluetooth operation failed")]
    Failed,
    #[error("Bluetooth operation in progress")]
    InProgress,
    #[error("Bluetooth operation not permitted")]
    NotPermitted,
    #[error("Bluetooth device not connected")]
    NotConnected,
    #[error("Bluetooth operation not supported")]
    NotSupported,
}

impl From<NotifyError> for dbus::Error {
    fn from(err: NotifyError) -> Self {
        let name: &'static str = err.clone().into();
        Self::new_custom(ERR_PREFIX.to_string() + name, &err.to_string())
    }
}

/// Local GATT characteristic exposed over Bluetooth.
pub struct Characteristic {
    /// 128-bit characteristic UUID.
    pub uuid: Uuid,
    /// Characteristic flags.
    pub flags: CharacteristicFlags,
    /// Characteristic descriptors.
    pub descriptors: Vec<CharacteristicDescriptor>,
    /// Read value of characteristic.
    pub read_value: Option<Box<dyn Fn(ReadValueRequest) -> Result<Vec<u8>, ReadValueError>>>,
    /// Write value of characteristic.
    pub write_value: Option<Box<dyn Fn(Vec<u8>, WriteValueRequest) -> Result<(), WriteValueError>>>,
    /// Request value change notifications over provided channel.
    pub notify: Option<Box<dyn Fn(mpsc::Sender<()>) -> Result<(), NotifyError>>>,
    // TODO: file descriptors
}

/// Local Bluetooth GATT characteristic flags.
pub struct CharacteristicFlags {
    pub broadcast: bool,
    pub read: bool,
    pub write_without_response: bool,
    pub write: bool,
    pub notify: bool,
    pub indicate: bool,
    pub authenticated_signed_writes: bool,
    pub extended_properties: bool,
    pub reliable_write: bool,
    pub writable_auxiliaries: bool,
    pub encrypt_read: bool,
    pub encrypt_write: bool,
    pub encrypt_authenticated_read: bool,
    pub encrypt_authenticated_write: bool,
    pub secure_read: bool,
    pub secure_write: bool,
    pub authorize: bool,
}

/// Local GATT characteristic descriptor exposed over Bluetooth.
pub struct CharacteristicDescriptor {
    /// 128-bit descriptor UUID.
    pub uuid: Uuid,
    /// Characteristic descriptor flags.
    pub flags: CharacteristicDescriptorFlags,
}

/// Local Bluetooth GATT characteristic flags.
pub struct CharacteristicDescriptorFlags {
    pub read: bool,
    pub write: bool,
    pub encrypt_read: bool,
    pub encrypt_write: bool,
    pub encrypt_authenticated_read: bool,
    pub encrypt_authenticated_write: bool,
    pub secure_read: bool,
    pub secure_write: bool,
    pub authorize: bool,
}
