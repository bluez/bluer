use crate::{
    all_dbus_objects, device::Device, Address, AddressType, Modalias, ObjectEvent, SERVICE_NAME, TIMEOUT,
};
//use crate::bluetooth_le_advertising_data::BluetoothAdvertisingData;
use crate::{device, session::Session, Error, Result};
use dbus::{
    nonblock::{
        stdintf::org_freedesktop_dbus::{
            ObjectManager, ObjectManagerInterfacesAdded, ObjectManagerInterfacesRemoved,
        },
        Proxy, SyncConnection,
    },
    Path,
};
use futures::{stream, Stream, StreamExt};
use std::{
    collections::HashMap,
    fmt::{Debug, Formatter},
    sync::Arc,
    u32,
};

pub(crate) const INTERFACE: &str = "org.bluez.Adapter1";
pub(crate) const PREFIX: &str = "/org/bluez/";

/// Interface to a Bluetooth adapter.
#[derive(Clone)]
pub struct Adapter {
    connection: Arc<SyncConnection>,
    dbus_path: Path<'static>,
    name: Arc<String>,
}

impl Debug for Adapter {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        write!(f, "Adapter {{ name: {} }}", self.name())
    }
}

/// Bluetooth device event.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeviceEvent {
    /// Device added.
    Added(Address),
    /// Device removed.
    Removed(Address),
}

impl Adapter {
    /// Create Bluetooth adapter interface for adapter with specified name.
    pub(crate) fn new(connection: Arc<SyncConnection>, name: &str) -> Result<Self> {
        Ok(Self {
            connection,
            dbus_path: Path::new(PREFIX.to_string() + name).map_err(|_| Error::InvalidName(name.to_string()))?,
            name: Arc::new(name.to_string()),
        })
    }

    fn proxy(&self) -> Proxy<'_, &SyncConnection> {
        Proxy::new(SERVICE_NAME, &self.dbus_path, TIMEOUT, &*self.connection)
    }

    pub(crate) fn parse_dbus_path<'a>(path: &'a Path) -> Option<&'a str> {
        path.strip_prefix(PREFIX)
    }

    /// The Bluetooth adapter name.
    ///
    /// For example hci0.
    pub fn name(&self) -> &str {
        &self.name
    }

    // pub async fn get_addata(&self) -> Result<BluetoothAdvertisingData<'_>> {
    //     let addata = bluetooth_utils::list_addata_1(&self.session.get_connection(), &self.object_path).await?;
    //
    //     if addata.is_empty() {
    //         return Err(Box::from("No addata found."));
    //     }
    //     Ok(BluetoothAdvertisingData::new(&self.session, &addata[0]))
    // }

    /// Bluetooth addresses of discovered Bluetooth devices.
    pub async fn device_addresses(&self) -> Result<Vec<Address>> {
        let mut addrs = Vec::new();
        for (path, interfaces) in all_dbus_objects(&*self.connection).await? {
            match Device::parse_dbus_path(&path) {
                Some((adapter, addr)) if adapter == *self.name && interfaces.contains_key(device::INTERFACE) => {
                    addrs.push(addr)
                }
                _ => (),
            }
        }
        Ok(addrs)
    }

    /// Get interface to Bluetooth device of specified address.
    pub fn device(&self, address: Address) -> Result<Device> {
        Device::new(self.connection.clone(), self.name.clone(), address)
    }

    /// Stream device added and removed events.
    pub async fn device_events(&self) -> Result<impl Stream<Item = DeviceEvent>> {
        let adapter_path = self.dbus_path.clone().into_static();
        let obj_events = ObjectEvent::stream(self.connection.clone(), Some(adapter_path.clone())).await?;

        let my_name = self.name.clone();
        let events = obj_events.filter_map(move |evt| {
            let my_name = my_name.clone();
            async move {
                match evt {
                    ObjectEvent::Added { object, .. } => match Device::parse_dbus_path(&object) {
                        Some((adapter, address)) if adapter == *my_name => Some(DeviceEvent::Added(address)),
                        _ => None,
                    },
                    ObjectEvent::Removed { object, .. } => match Device::parse_dbus_path(&object) {
                        Some((adapter, address)) if adapter == *my_name => Some(DeviceEvent::Removed(address)),
                        _ => None,
                    },
                }
            }
        });
        Ok(events)
    }

    dbus_interface!(INTERFACE);

    // ===========================================================================================
    // Properties
    // ===========================================================================================

    define_property!(
        /// The Bluetooth device address.
        address, "Address" => String
    );

    /// The Bluetooth Address Type.
    ///
    /// For dual-mode and BR/EDR
    /// only adapter this defaults to "public". Single mode LE
    /// adapters may have either value. With privacy enabled
    /// this contains type of Identity Address and not type of
    /// address used for connection.
    pub async fn address_type(&self) -> Result<AddressType> {
        let address_type: String = self.get_property("AddressType").await?;
        Ok(address_type.parse()?)
    }

    define_property!(
        ///	The Bluetooth system name (pretty hostname).
        ///
        /// This property is either a static system default
        /// or controlled by an external daemon providing
        /// access to the pretty hostname configuration.
        system_name, "Name" => String
    );

    define_property!(
        /// The Bluetooth friendly name.
        ///
        /// This value can be changed.
        ///
        /// In case no alias is set, it will return the system
        /// provided name. Setting an empty string as alias will
        /// convert it back to the system provided name.
        ///
        /// When resetting the alias with an empty string, the
        /// property will default back to system name.
        ///
        /// On a well configured system, this property never
        /// needs to be changed since it defaults to the system
        /// name and provides the pretty hostname. Only if the
        /// local name needs to be different from the pretty
        /// hostname, this property should be used as last
        /// resort.
        alias, set_alias, "Alias" => String
    );

    define_property!(
        /// The Bluetooth class of device.
        ///
        ///	This property represents the value that is either
        ///	automatically configured by DMI/ACPI information
        ///	or provided as static configuration.
        class, "Class" => u32
    );

    define_property!(
        /// Switch an adapter on or off. This will also set the
        /// appropriate connectable state of the controller.
        ///
        /// The value of this property is not persistent. After
        /// restart or unplugging of the adapter it will reset
        /// back to false.
        is_powered, set_powered, "Powered" => bool
    );

    define_property!(
        /// Switch an adapter to discoverable or non-discoverable
        /// to either make it visible or hide it.
        ///
        /// This is a global
        /// setting and should only be used by the settings
        /// application.
        ///
        /// If the DiscoverableTimeout is set to a non-zero
        /// value then the system will set this value back to
        /// false after the timer expired.
        ///
        /// In case the adapter is switched off, setting this
        /// value will fail.
        ///
        /// When changing the Powered property the new state of
        /// this property will be updated via a PropertiesChanged
        /// signal.
        ///
        /// For any new adapter this settings defaults to false.
        is_discoverable, set_discoverable, "Discoverable" => bool
    );

    define_property!(
        /// Switch an adapter to pairable or non-pairable.
        ///
        /// This is
        /// a global setting and should only be used by the
        /// settings application.
        ///
        /// Note that this property only affects incoming pairing
        /// requests.
        ///
        /// For any new adapter this settings defaults to true.
        is_pairable, set_pairable, "Pairable" => bool
    );

    define_property!(
        /// The pairable timeout in seconds.
        ///
        /// A value of zero
        /// means that the timeout is disabled and it will stay in
        /// pairable mode forever.
        ///
        /// The default value for pairable timeout should be
        /// disabled (value 0).
        pairable_timeout, set_pairable_timeout, "PairableTimeout" => u32
    );

    define_property!(
        /// The discoverable timeout in seconds.
        ///
        /// A value of zero
        /// means that the timeout is disabled and it will stay in
        /// discoverable/limited mode forever.
        ///
        /// The default value for the discoverable timeout should
        /// be 180 seconds (3 minutes).
        discoverable_timeout, set_discoverable_timeout, "DiscoverableTimeout" => u32
    );

    define_property!(
        ///	Indicates that a device discovery procedure is active.
        is_discovering, "Discovering" => bool
    );

    define_property!(
        /// List of 128-bit UUIDs that represents the available
        /// lcal services.
        uuids, "UUIDs" => Vec<String>
    );

    /// Local Device ID information in modalias format
    /// used by the kernel and udev.
    pub async fn modalias(&self) -> Result<Modalias> {
        let modalias: String = self.get_property("Modalias").await?;
        Ok(modalias.parse()?)
    }

    // ===========================================================================================
    // Methods
    // ===========================================================================================

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/adapter-api.txt#n12
    // Don't use this method, it's just a bomb now.
    //pub fn start_discovery(&self) -> Result<()> {
    //    Err(Box::from("Deprecated, use Discovery Session"))
    //}

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/adapter-api.txt#n27
    // Don't use this method, it's just a bomb now.
    //pub fn stop_discovery(&self) -> Result<()> {
    //    Err(Box::from("Deprecated, use Discovery Session"))
    //}

    /// This removes the remote device object at the given
    /// path.
    ///
    /// It will remove also the pairing information.
    pub async fn remove_device(&self, device: &str) -> Result<()> {
        self.call_method("RemoveDevice", (Path::from(device),)).await?;
        Ok(())
    }

    /// This method connects to device without need of
    /// performing General Discovery.
    ///
    /// Connection mechanism is
    /// similar to Connect method from Device1 interface with
    /// exception that this method returns success when physical
    /// connection is established. After this method returns,
    /// services discovery will continue and any supported
    /// profile will be connected. There is no need for calling
    /// Connect on Device1 after this call. If connection was
    /// successful this method returns object path to created
    /// device object.
    ///
    /// Parameters that may be set in the filter dictionary
    /// include the following:    
    ///
    ///  `address` -
    ///     The Bluetooth device address of the remote
    ///     device. This parameter is mandatory.
    ///
    /// `address_type` -
    ///     The Bluetooth device Address Type. This is
    ///     address type that should be used for initial
    ///     connection. If this parameter is not present
    ///     BR/EDR device is created.    
    pub async fn connect_device(
        &self, address: Address, address_type: Option<AddressType>,
    ) -> Result<Path<'static>> {
        let mut m = HashMap::new();
        m.insert("Address", address.to_string());
        if let Some(address_type) = address_type {
            m.insert("AddressType", address_type.to_string());
        }
        let (path,): (Path,) = self.call_method("ConnectDevice", (m,)).await?;
        Ok(path)
    }
}
