use dbus::{
    nonblock::{Proxy, SyncConnection},
    Path,
};
use std::{
    collections::{HashMap, HashSet},
    fmt,
    sync::Arc,
};
use uuid::Uuid;

use crate::{adapter, Address, AddressType, Error, Modalias, Result, SERVICE_NAME, TIMEOUT};

pub(crate) const INTERFACE: &str = "org.bluez.Device1";

/// Interface to a Bluetooth device.
#[derive(Clone)]
pub struct Device {
    connection: Arc<SyncConnection>,
    dbus_path: Path<'static>,
    adapter_name: Arc<String>,
    address: Address,
}

impl fmt::Debug for Device {
    fn fmt(&self, f: &mut fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "Device {{ adapter_name: {}, address: {} }}",
            self.adapter_name(),
            self.address()
        )
    }
}

impl Device {
    /// Create Bluetooth device interface for device of specified address connected to specified adapater.
    pub(crate) fn new(
        connection: Arc<SyncConnection>,
        adapter_name: Arc<String>,
        address: Address,
    ) -> Result<Self> {
        Ok(Self {
            connection,
            dbus_path: Path::new(format!(
                "{}{}/dev_{}",
                adapter::PREFIX,
                adapter_name,
                address.to_string().replace(':', "_")
            ))
            .map_err(|_| Error::InvalidName((*adapter_name).clone()))?,
            adapter_name,
            address,
        })
    }

    fn proxy(&self) -> Proxy<'_, &SyncConnection> {
        Proxy::new(SERVICE_NAME, &self.dbus_path, TIMEOUT, &*self.connection)
    }

    pub(crate) fn parse_dbus_path(path: &Path<'static>) -> Option<(String, Address)> {
        match path.strip_prefix(adapter::PREFIX) {
            Some(adapter_dev) => match adapter_dev.split("/dev_").collect::<Vec<_>>().as_slice() {
                &[adapater, dev] => match dev.replace('_', ":").parse() {
                    Ok(addr) => (Some((adapater.to_string(), addr))),
                    Err(_) => None,
                },
                _ => None,
            },
            None => None,
        }
    }

    /// The Bluetooth adapter name.
    pub fn adapter_name(&self) -> &str {
        &self.adapter_name
    }

    /// The Bluetooth device address of the remote device.
    pub fn address(&self) -> Address {
        self.address
    }

    //     pub async fn get_addata(&self) -> Result<BluetoothAdvertisingData<'_>> {
    //         let addata = bluetooth_utils::list_addata_2(&self.session.connection(), &self.object_path).await?;
    //
    //         if addata.is_empty() {
    //             return Err(Box::from("No addata found."));
    //         }
    //         Ok(BluetoothAdvertisingData::new(&self.session, &addata[0]))
    //     }

    dbus_interface!(INTERFACE);

    // ===========================================================================================
    // Properties
    // ===========================================================================================

    /// The Bluetooth device Address Type.
    ///
    /// For dual-mode and
    /// BR/EDR only devices this defaults to "public". Single
    /// mode LE devices may have either value. If remote device
    /// uses privacy than before pairing this represents address
    /// type used for connection and Identity Address after
    /// pairing.
    pub async fn address_type(&self) -> Result<AddressType> {
        let address_type: String = self.get_property("AddressType").await?;
        Ok(address_type.parse()?)
    }

    define_property!(
        /// The Bluetooth remote name.
        ///
        /// This value can not be
        ///	changed. Use the Alias property instead.
        ///
        ///	This value is only present for completeness. It is
        ///	better to always use the Alias property when
        ///	displaying the devices name.
        ///
        ///	If the Alias property is unset, it will reflect
        ///	this value which makes it more convenient.
        name, "Name" => Option<String>
    );

    define_property!(
        /// Proposed icon name according to the freedesktop.org
        /// icon naming specification.
        icon, "Icon" => Option<String>
    );

    define_property!(
        ///	The Bluetooth class of device of the remote device.
        class, "Class" => Option<u32>
    );

    define_property!(
        ///	External appearance of device, as found on GAP service.
        appearance, "Appearance" => Option<u32>
    );

    ///	List of 128-bit UUIDs that represents the available
    /// remote services.    
    pub async fn uuids(&self) -> Result<Option<HashSet<Uuid>>> {
        let uuids: Vec<String> = match self.get_opt_property("UUIDs").await? {
            Some(uuids) => uuids,
            None => return Ok(None),
        };
        let uuids: HashSet<Uuid> = uuids
            .into_iter()
            .map(|uuid| {
                uuid.parse()
                    .map_err(|_| Error::InvalidUuid(uuid.to_string()))
            })
            .collect::<Result<HashSet<Uuid>>>()?;
        Ok(Some(uuids))
    }

    define_property!(
        ///	Indicates if the remote device is paired.
        is_paired, "Paired" => bool
    );

    define_property!(
        ///	Indicates if the remote device is paired.
        is_connected, "Connected" => bool
    );

    // /// True, when connected and paired.
    // pub async fn is_ready_to_receive(&self) -> bool {
    //     let is_connected: bool = self.is_connected().await.unwrap_or(false);
    //     let is_paired: bool = self.is_paired().await.unwrap_or(false);
    //     is_paired && is_connected
    // }

    define_property!(
        ///	Indicates if the remote is seen as trusted. This
        /// setting can be changed by the application.
        is_trusted, set_trusted, "Trusted" => bool
    );

    define_property!(
        /// If set to true any incoming connections from the
        /// device will be immediately rejected.
        ///
        /// Any device
        /// drivers will also be removed and no new ones will
        /// be probed as long as the device is blocked.
        is_blocked, set_blocked, "Blocked" => bool
    );

    define_property!(
        /// If set to true this device will be allowed to wake the
        /// host from system suspend.
        is_wake_allowed, set_wake_allowed, "WakeAllowed" => bool
    );

    define_property!(
        /// The name alias for the remote device.
        ///
        /// The alias can
        /// be used to have a different friendly name for the
        /// remote device.
        ///
        /// In case no alias is set, it will return the remote
        /// device name. Setting an empty string as alias will
        /// convert it back to the remote device name.
        ///
        /// When resetting the alias with an empty string, the
        /// property will default back to the remote name.
        alias, set_alias, "Alias" => String
    );

    // define_property!(
    //     /// The object path of the adapter the device belongs to.
    //     adapter, "Adapter" => String
    // );

    define_property!(
        /// Set to true if the device only supports the pre-2.1
        /// pairing mechanism.
        ///
        /// This property is useful during
        /// device discovery to anticipate whether legacy or
        /// simple pairing will occur if pairing is initiated.
        ///
        /// Note that this property can exhibit false-positives
        /// in the case of Bluetooth 2.1 (or newer) devices that
        /// have disabled Extended Inquiry Response support.
        is_legacy_pairing, "LegacyPairing" => String
    );

    /// Remote Device ID information in modalias format
    /// used by the kernel and udev.
    pub async fn modalias(&self) -> Result<Option<Modalias>> {
        let modalias: String = match self.get_opt_property("Modalias").await? {
            Some(modalias) => modalias,
            None => return Ok(None),
        };
        Ok(Some(modalias.parse()?))
    }

    define_property!(
        /// Received Signal Strength Indicator of the remote
        ///	device (inquiry or advertising).
        rssi, "RSSI" => Option<i16>
    );

    define_property!(
        /// Advertised transmitted power level (inquiry or
        /// advertising).
        tx_power, "TxPower" => Option<i16>
    );

    define_property!(
        /// Manufacturer specific advertisement data.
        ///
        /// Keys are
        /// 16 bits Manufacturer ID followed by its byte array
        /// value.
        manufacturer_data, "ManufacturerData" => Option<HashMap<u16, Vec<u8>>>
    );

    define_property!(
        /// Service advertisement data.
        ///
        /// Keys are the UUIDs in
        /// string format followed by its byte array value.
        service_data, "ServiceData" => Option<HashMap<String, Vec<u8>>>
    );

    define_property!(
        /// Indicate whether or not service discovery has been
        /// resolved.
        is_services_resolved, "ServicesResolved " => bool
    );

    define_property!(
        /// The Advertising Data Flags of the remote device.
        advertising_flags, "AdvertisingFlags" => Vec<u8>
    );

    define_property!(
        /// The Advertising Data of the remote device.
        ///
        /// Note: Only types considered safe to be handled by
        /// application are exposed.
        advertising_data, "AdvertisingData" => HashMap<u8, Vec<u8>>
    );

    // pub async fn get_gatt_services(&self) -> Result<Vec<String>> {
    //     bluetooth_utils::list_services(&self.session.connection(), &self.object_path).await
    // }

    // ===========================================================================================
    // Methods
    // ===========================================================================================

    /// This is a generic method to connect any profiles
    /// the remote device supports that can be connected
    /// to and have been flagged as auto-connectable on
    /// our side.
    ///
    /// If only subset of profiles is already
    /// connected it will try to connect currently disconnected
    /// ones.
    ///
    /// If at least one profile was connected successfully this
    /// method will indicate success.
    ///
    /// For dual-mode devices only one bearer is connected at
    /// time, the conditions are in the following order:
    ///
    ///     1. Connect the disconnected bearer if already
    ///     connected.
    ///
    ///     2. Connect first the bonded bearer. If no
    ///     bearers are bonded or both are skip and check
    ///     latest seen bearer.
    ///
    ///     3. Connect last seen bearer, in case the
    ///     timestamps are the same BR/EDR takes
    ///     precedence.
    pub async fn connect(&self) -> Result<()> {
        self.call_method("Connect", ()).await
    }

    /// This method gracefully disconnects all connected
    /// profiles and then terminates low-level ACL connection.
    ///
    /// ACL connection will be terminated even if some profiles
    /// were not disconnected properly e.g. due to misbehaving
    /// device.
    ///
    /// This method can be also used to cancel a preceding
    /// Connect call before a reply to it has been received.
    ///
    /// For non-trusted devices connected over LE bearer calling
    /// this method will disable incoming connections until
    /// Connect method is called again.
    pub async fn disconnect(&self) -> Result<()> {
        self.call_method("Disconnect", ()).await
    }

    /// This method connects a specific profile of this
    /// device. The UUID provided is the remote service
    /// UUID for the profile.
    pub async fn connect_profile(&self, uuid: &str) -> Result<()> {
        self.call_method("ConnectProfile", (uuid,)).await
    }

    /// This method disconnects a specific profile of
    /// this device.
    ///
    /// The profile needs to be registered
    /// client profile.
    ///
    /// There is no connection tracking for a profile, so
    /// as long as the profile is registered this will always
    /// succeed.
    pub async fn disconnect_profile(&self, uuid: &str) -> Result<()> {
        self.call_method("DisconnectProfile", (uuid,)).await
    }

    /// This method will connect to the remote device,
    /// initiate pairing and then retrieve all SDP records
    /// (or GATT primary services).
    ///
    /// If the application has registered its own agent,
    /// then that specific agent will be used. Otherwise
    /// it will use the default agent.
    ///
    /// Only for applications like a pairing wizard it
    /// would make sense to have its own agent. In almost
    /// all other cases the default agent will handle
    /// this just fine.
    ///
    /// In case there is no application agent and also
    /// no default agent present, this method will fail.
    pub async fn pair(&self) -> Result<()> {
        self.call_method("Pair", ()).await
    }

    /// This method can be used to cancel a pairing
    /// operation initiated by the Pair method.
    pub async fn cancel_pairing(&self) -> Result<()> {
        self.call_method("CancelPairing", ()).await
    }
}
