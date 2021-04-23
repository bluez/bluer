use std::{
    convert::TryInto,
    fmt::{Debug, Display, Formatter},
    str::FromStr,
    time::Duration,
};

pub use crate::adapter::Adapter;
pub use crate::bluetooth_device::BluetoothDevice;
pub use crate::bluetooth_discovery_session::BluetoothDiscoverySession;
pub use crate::bluetooth_event::BluetoothEvent;
pub use crate::bluetooth_gatt_characteristic::BluetoothGATTCharacteristic;
pub use crate::bluetooth_gatt_descriptor::BluetoothGATTDescriptor;
pub use crate::bluetooth_gatt_service::BluetoothGATTService;
pub use crate::bluetooth_le_advertising_data::BluetoothAdvertisingData;
pub use crate::bluetooth_le_advertising_manager::BluetoothAdvertisingManager;
pub use crate::bluetooth_obex::BluetoothOBEXSession;
pub use crate::session::Session;

use thiserror::Error;
use tokio::task::JoinError;

macro_rules! other_err {
    ($($e:tt)*) => {
        crate::Error::Other(format!($($e)*))
    };
}

pub(crate) const SERVICE_NAME: &str = "org.bluez";
pub(crate) const TIMEOUT: Duration = Duration::from_secs(120);

macro_rules! dbus_interface {
    ($interface:expr) => {
        async fn get_property<R>(&self, name: &str) -> crate::Result<R>
        where
            R: for<'b> dbus::arg::Get<'b> + 'static,
        {
            use dbus::nonblock::stdintf::org_freedesktop_dbus::Properties;
            Ok(self.proxy.get($interface, name).await?)
        }

        async fn set_property<T>(&self, name: &str, value: T) -> crate::Result<()>
        where
            T: dbus::arg::Arg + dbus::arg::Append,
        {
            use dbus::nonblock::stdintf::org_freedesktop_dbus::Properties;
            self.proxy.set($interface, name, value).await?;
            Ok(())
        }

        async fn call_method<A, R>(&self, name: &str, args: A) -> crate::Result<R>
        where
            A: dbus::arg::AppendAll,
            R: dbus::arg::ReadAll + 'static,
        {
            Ok(self.proxy.method_call($interface, name, args).await?)
        }
    };
}

macro_rules! define_property {
    ($(#[$outer:meta])* $getter_name:ident, $dbus_name:expr => $type:ty) => {
        $(#[$outer])*
        pub async fn $getter_name(&self) -> crate::Result<$type> {
            self.get_property($dbus_name).await
        }
    };

    ($(#[$outer:meta])* $getter_name:ident, $setter_name:ident, $dbus_name:expr => $type:ty) => {
        $(#[$outer])*
        pub async fn $getter_name(&self) -> crate::Result<$type> {
            self.get_property($dbus_name).await
        }
        $(#[$outer])*
        pub async fn $setter_name(&self, value: $type) -> crate::Result<()> {
            self.set_property($dbus_name, value).await;
            Ok(())
        }
    };
}

mod adapter;
mod bluetooth_device;
mod bluetooth_discovery_session;
mod bluetooth_event;
mod bluetooth_gatt_characteristic;
mod bluetooth_gatt_descriptor;
mod bluetooth_gatt_service;
mod bluetooth_le_advertising_data;
mod bluetooth_le_advertising_manager;
mod bluetooth_obex;
mod bluetooth_utils;
mod session;

/// Bluetooth error.
#[derive(Clone, Debug, Error)]
pub enum Error {
    #[error("Bluetooth device already connected")]
    AlreadyConnected,
    #[error("Bluetooth device already exists")]
    AlreadyExists,
    #[error("Bluetooth authentication canceled")]
    AuthenticationCanceled,
    #[error("Bluetooth authentication failed")]
    AuthenticationFailed,
    #[error("Bluetooth authentication rejected")]
    AuthenticationRejected,
    #[error("Bluetooth authentication timeout")]
    AuthenticationTimeout,
    #[error("Bluetooth connection attempt failed")]
    ConnectionAttemptFailed,
    #[error("Bluetooth device does not exist")]
    DoesNotExist,
    #[error("Bluetooth operation failed")]
    Failed,
    #[error("Bluetooth operation in progress")]
    InProgress,
    #[error("invalid arguments for Bluetooth operation")]
    InvalidArguments,
    #[error("Bluetooth operation not available")]
    NotAvailable,
    #[error("Bluetooth device not ready")]
    NotReady,
    #[error("Bluetooth operation not supported")]
    NotSupported,
    #[error("Bluetooth D-Bus error {name}: {message}")]
    DBus { name: String, message: String },
    #[error("No Bluetooth adapter available")]
    NoAdapterAvailable,
    #[error("Bluetooth adapter {0} is not available")]
    AdapterNotAvailable(String),
    #[error("Join error: {0}")]
    JoinError(String),
    #[error("Invalid Bluetooth address: {0}")]
    InvalidAddress(String),
    #[error("Bluetooth error: {0}")]
    Other(String),
}

impl From<dbus::Error> for Error {
    fn from(err: dbus::Error) -> Self {
        match err.name().and_then(|name| name.strip_prefix("org.bluez.Error.")) {
            Some("AlreadyConnected") => Self::AlreadyConnected,
            Some("AlreadyExists") => Self::AlreadyExists,
            Some("AuthenticationCanceled") => Self::AuthenticationCanceled,
            Some("AuthenticationFailed") => Self::AuthenticationFailed,
            Some("AuthenticationRejected") => Self::AuthenticationRejected,
            Some("AuthenticationTimeout") => Self::AuthenticationTimeout,
            Some("ConnectionAttemptFailed") => Self::ConnectionAttemptFailed,
            Some("DoesNotExist") => Self::DoesNotExist,
            Some("Failed") => Self::Failed,
            Some("InProgress") => Self::InProgress,
            Some("InvalidArguments") => Self::InvalidArguments,
            Some("NotAvailable") => Self::NotAvailable,
            Some("NotReady") => Self::NotReady,
            Some("NotSupported") => Self::NotSupported,
            _ => Self::DBus {
                name: err.name().unwrap_or_default().to_string(),
                message: err.message().unwrap_or_default().to_string(),
            },
        }
    }
}

impl From<JoinError> for Error {
    fn from(err: JoinError) -> Self {
        Self::JoinError(err.to_string())
    }
}

/// Bluetooth result.
pub type Result<T> = std::result::Result<T, Error>;

/// Bluetooth address.
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Address([u8; 6]);

impl Display for Address {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        write!(
            f,
            "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
            self.0[0], self.0[1], self.0[2], self.0[3], self.0[4], self.0[5]
        )
    }
}

impl Debug for Address {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}

impl FromStr for Address {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        let fields = s
            .split(':')
            .map(|s| u8::from_str_radix(s, 16).map_err(|_| Error::InvalidAddress(s.to_string())))
            .collect::<Result<Vec<_>>>()?;
        Ok(Self(fields.try_into().map_err(|_| Error::InvalidAddress(s.to_string()))?))
    }
}

/// Bluetooth device address type.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum AddressType {
    /// Public address
    Public,
    /// Random address
    Random,
}

impl FromStr for AddressType {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "public" => Ok(Self::Public),
            "random" => Ok(Self::Random),
            _ => Err(other_err!("unknown address type: {}", &s)),
        }
    }
}
