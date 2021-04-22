pub use crate::bluetooth_adapter::BluetoothAdapter;
pub use crate::bluetooth_device::BluetoothAddressType;
pub use crate::bluetooth_device::BluetoothDevice;
pub use crate::bluetooth_discovery_session::BluetoothDiscoverySession;
pub use crate::bluetooth_event::BluetoothEvent;
pub use crate::bluetooth_gatt_characteristic::BluetoothGATTCharacteristic;
pub use crate::bluetooth_gatt_descriptor::BluetoothGATTDescriptor;
pub use crate::bluetooth_gatt_service::BluetoothGATTService;
pub use crate::bluetooth_le_advertising_data::BluetoothAdvertisingData;
pub use crate::bluetooth_le_advertising_manager::BluetoothAdvertisingManager;
pub use crate::bluetooth_obex::BluetoothOBEXSession;
pub use crate::bluetooth_session::BluetoothSession;

use thiserror::Error;

macro_rules! dbus_methods {
    ($interface:expr) => {
        async fn get_property<R>(&self, prop: &str) -> Result<R, Box<dyn Error>>
        where
            R: for<'b> dbus::arg::Get<'b> + 'static,
        {
            bluetooth_utils::get_property(&self.session.get_connection(), $interface, &self.object_path, prop)
                .await
        }

        async fn set_property<T>(&self, prop: &str, value: T, timeout_ms: i32) -> Result<(), Box<dyn Error>>
        where
            T: dbus::arg::Arg + dbus::arg::Append,
        {
            bluetooth_utils::set_property(
                &self.session.get_connection(),
                $interface,
                &self.object_path,
                prop,
                value,
                timeout_ms,
            )
            .await
        }

        async fn call_method<A, R>(&self, method: &str, param: A, timeout_ms: i32) -> Result<R, Box<dyn Error>>
        where
            A: dbus::arg::AppendAll,
            R: dbus::arg::ReadAll + 'static,
        {
            bluetooth_utils::call_method(
                &self.session.get_connection(),
                $interface,
                &self.object_path,
                method,
                param,
                timeout_ms,
            )
            .await
        }
    };
}

macro_rules! define_property {
    ($(#[$outer:meta])* $getter_name:ident, $dbus_name:expr => $type:ty) => {
        $(#[$outer])*
        pub async fn $getter_name(&self) -> Result<$type, Box<dyn Error>> {
            self.get_property($dbus_name).await
        }
    };

    ($(#[$outer:meta])* $getter_name:ident, $setter_name:ident, $dbus_name:expr => $type:ty) => {
        $(#[$outer])*
        pub async fn $getter_name(&self) -> Result<$type, Box<dyn Error>> {
            self.get_property($dbus_name).await
        }
        $(#[$outer])*
        pub async fn $setter_name(&self, value: $type) -> Result<(), Box<dyn Error>> {
            self.set_property($dbus_name, value, 1000).await;
            Ok(())
        }
    };
}

mod bluetooth_adapter;
mod bluetooth_device;
mod bluetooth_discovery_session;
mod bluetooth_event;
mod bluetooth_gatt_characteristic;
mod bluetooth_gatt_descriptor;
mod bluetooth_gatt_service;
mod bluetooth_le_advertising_data;
mod bluetooth_le_advertising_manager;
mod bluetooth_obex;
mod bluetooth_session;
mod bluetooth_utils;

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
    #[error("{0}")]
    Other(String),
}

/// Bluetooth result.
pub type Result<T> = std::result::Result<T, Error>;
