//! Bluetooth adapter.

use dbus::{
    arg::{PropMap, RefArg, Variant},
    nonblock::{Proxy, SyncConnection},
    Path,
};
use futures::{
    stream::{self, SelectAll},
    Stream, StreamExt,
};
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    fmt::{Debug, Formatter},
    sync::Arc,
    u32,
};
use strum::{Display, EnumString};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

use crate::{
    adv,
    adv::{Advertisement, AdvertisementHandle, Capabilities, Feature, PlatformFeature, SecondaryChannel},
    all_dbus_objects, device,
    device::Device,
    gatt, Address, AddressType, Error, ErrorKind, Event, InternalErrorKind, Modalias, Result, SessionInner,
    SingleSessionToken, SERVICE_NAME, TIMEOUT,
};

pub(crate) const INTERFACE: &str = "org.bluez.Adapter1";
pub(crate) const PATH: &str = "/org/bluez";
pub(crate) const PREFIX: &str = "/org/bluez/";

/// Default adapter name.
pub(crate) const DEFAULT_NAME: &str = "hci0";

/// Interface to a Bluetooth adapter.
#[cfg_attr(docsrs, doc(cfg(feature = "bluetoothd")))]
#[derive(Clone)]
pub struct Adapter {
    inner: Arc<SessionInner>,
    dbus_path: Path<'static>,
    name: Arc<String>,
}

impl Debug for Adapter {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        write!(f, "Adapter {{ name: {} }}", self.name())
    }
}

impl Adapter {
    /// Create Bluetooth adapter interface for adapter with specified name.
    pub(crate) fn new(inner: Arc<SessionInner>, name: &str) -> Result<Self> {
        Ok(Self {
            inner,
            dbus_path: Path::new(PREFIX.to_string() + name)
                .map_err(|_| Error::new(ErrorKind::InvalidName(name.to_string())))?,
            name: Arc::new(name.to_string()),
        })
    }

    fn proxy(&self) -> Proxy<'_, &SyncConnection> {
        Proxy::new(SERVICE_NAME, &self.dbus_path, TIMEOUT, &*self.inner.connection)
    }

    pub(crate) fn dbus_path(adapter_name: &str) -> Result<Path<'static>> {
        Path::new(format!("{}{}", PREFIX, adapter_name,))
            .map_err(|_| Error::new(ErrorKind::InvalidName((*adapter_name).to_string())))
    }

    pub(crate) fn parse_dbus_path_prefix<'a>(path: &'a Path) -> Option<(&'a str, &'a str)> {
        match path.strip_prefix(PREFIX) {
            Some(p) => {
                let sep = p.find('/').unwrap_or(p.len());
                Some((&p[0..sep], &p[sep..]))
            }
            None => None,
        }
    }

    pub(crate) fn parse_dbus_path<'a>(path: &'a Path) -> Option<&'a str> {
        match Self::parse_dbus_path_prefix(path) {
            Some((v, "")) => Some(v),
            _ => None,
        }
    }

    /// The Bluetooth adapter name.
    ///
    /// For example `hci0`.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Bluetooth addresses of discovered Bluetooth devices.
    pub async fn device_addresses(&self) -> Result<Vec<Address>> {
        let mut addrs = Vec::new();
        for (path, interfaces) in all_dbus_objects(&*self.inner.connection).await? {
            match Device::parse_dbus_path(&path) {
                Some((adapter, addr)) if adapter == *self.name && interfaces.contains_key(device::INTERFACE) => {
                    addrs.push(addr)
                }
                _ => (),
            }
        }
        Ok(addrs)
    }

    pub async fn register_monitor(&self, monitor: Monitor) -> Result<MonitorHandle> {
        let reg_monitor = RegisteredMonitor::new(monitor);
        reg_monitor.register(self.inner.clone(), self.name).await
    }


    /// Get interface to Bluetooth device of specified address.
    pub fn device(&self, address: Address) -> Result<Device> {
        Device::new(self.inner.clone(), self.name.clone(), address)
    }

    /// This method starts the device discovery session.
    ///
    /// This includes an inquiry procedure and remote device name resolving.
    ///
    /// This process will start streaming device addresses as new devices are discovered.
    /// A device may be discovered multiple times.
    ///
    /// All already known devices are also included in the device stream.
    /// This may include devices that are currently not in range.
    /// Check the [Device::rssi] property to see if the device is currently present.
    ///
    /// Device properties are queried asynchronously and may not be available
    /// yet when a [DeviceAdded event](AdapterEvent::DeviceAdded) occurs.
    /// Use [discover_devices_with_changes](Self::discover_devices_with_changes)
    /// when you want to be notified when the device properties change.
    pub async fn discover_devices(&self) -> Result<impl Stream<Item = AdapterEvent>> {
        let token = self.discovery_session().await?;
        let change_events = self.events().await?.map(move |evt| {
            let _token = &token;
            evt
        });

        let known = self.device_addresses().await?;
        let known_events = stream::iter(known).map(AdapterEvent::DeviceAdded);

        let all_events = known_events.chain(change_events);

        Ok(all_events)
    }

    /// This method starts the device discovery session and notifies of device property changes.
    ///
    /// This includes an inquiry procedure and remote device name resolving.
    ///
    /// This process will start streaming device addresses as new devices are discovered.
    /// Each time device properties change you will receive an additional
    /// [DeviceAdded event](AdapterEvent::DeviceAdded) for that device.
    ///
    /// All already known devices are also included in the device stream.
    /// This may include devices that are currently not in range.
    /// Check the [Device::rssi] property to see if the device is currently present.
    pub async fn discover_devices_with_changes(&self) -> Result<impl Stream<Item = AdapterEvent>> {
        let (tx, rx) = mpsc::channel(1);
        let mut discovery = self.discover_devices().await?;
        let adapter = self.clone();

        tokio::spawn(async move {
            let mut changes = SelectAll::new();

            loop {
                tokio::select! {
                    evt = discovery.next() => {
                        match evt {
                            Some(AdapterEvent::DeviceAdded(addr)) => {
                                if let Ok(dev) = adapter.device(addr) {
                                    if let Ok(dev_evts) = dev.events().await {
                                        changes.push(dev_evts.map(move |_| addr));
                                    }
                                }
                                let _ = tx.send(AdapterEvent::DeviceAdded(addr)).await;
                            },
                            Some(AdapterEvent::DeviceRemoved(addr)) => {
                                let _ = tx.send(AdapterEvent::DeviceRemoved(addr)).await;
                            },
                            Some(_) => (),
                            None => break,
                        }
                    },
                    Some(addr) = changes.next(), if !changes.is_empty() => {
                        let _ = tx.send(AdapterEvent::DeviceAdded(addr)).await;
                    },
                    () = tx.closed() => break,
                }
            }
        });

        Ok(ReceiverStream::new(rx))
    }

    async fn discovery_session(&self) -> Result<SingleSessionToken> {
        let dbus_path = self.dbus_path.clone();
        let connection = self.inner.connection.clone();
        self.inner
            .single_session(
                &self.dbus_path,
                async move {
                    let filter = DiscoveryFilter {
                        duplicate_data: false,
                        transport: DiscoveryTransport::Auto,
                        ..Default::default()
                    };
                    self.call_method("SetDiscoveryFilter", (filter.into_dict(),)).await?;
                    self.call_method("StartDiscovery", ()).await?;
                    Ok(())
                },
                async move {
                    log::trace!("{}: {}.StopDiscovery ()", &dbus_path, SERVICE_NAME);
                    let proxy = Proxy::new(SERVICE_NAME, &dbus_path, TIMEOUT, &*connection);
                    let result: std::result::Result<(), dbus::Error> =
                        proxy.method_call(INTERFACE, "StopDiscovery", ()).await;
                    log::trace!("{}: {}.StopDiscovery () -> {:?}", &dbus_path, SERVICE_NAME, &result);
                },
            )
            .await
    }

    dbus_interface!();
    dbus_default_interface!(INTERFACE);

    /// Streams adapter property and device changes.
    ///
    /// The stream ends when the adapter is removed.
    pub async fn events(&self) -> Result<impl Stream<Item = AdapterEvent>> {
        let name = self.name.clone();
        let events = self.inner.events(self.dbus_path.clone(), true).await?;
        let stream = events.flat_map(move |event| match event {
            Event::ObjectAdded { object, .. } => match Device::parse_dbus_path(&object) {
                Some((adapter, address)) if adapter == *name => {
                    stream::once(async move { AdapterEvent::DeviceAdded(address) }).boxed()
                }
                _ => stream::empty().boxed(),
            },
            Event::ObjectRemoved { object, .. } => match Device::parse_dbus_path(&object) {
                Some((adapter, address)) if adapter == *name => {
                    stream::once(async move { AdapterEvent::DeviceRemoved(address) }).boxed()
                }
                _ => stream::empty().boxed(),
            },
            Event::PropertiesChanged { changed, .. } => stream::iter(
                AdapterProperty::from_prop_map(changed).into_iter().map(AdapterEvent::PropertyChanged),
            )
            .boxed(),
        });
        Ok(stream)
    }

    /// Registers an advertisement object to be sent over the LE
    /// Advertising channel.
    ///
    /// InvalidArguments error indicates that the object has
    /// invalid or conflicting properties.
    ///
    /// InvalidLength error indicates that the data
    /// provided generates a data packet which is too long.
    ///
    /// The properties of this object are parsed when it is
    /// registered, and any changes are ignored.
    ///
    /// If the same object is registered twice it will result in
    /// an AlreadyExists error.
    ///
    /// If the maximum number of advertisement instances is
    /// reached it will result in NotPermitted error.
    ///
    /// Drop the returned [AdvertisementHandle] to unregister the advertisement.
    pub async fn advertise(&self, le_advertisement: Advertisement) -> Result<AdvertisementHandle> {
        le_advertisement.register(self.inner.clone(), self.name.clone()).await
    }

    /// Registers a local GATT services hierarchy (GATT Server).
    ///
    /// Registering a service allows applications to publish a *local* GATT service,
    /// which then becomes available to remote devices.
    ///
    /// Drop the returned [ApplicationHandle](gatt::local::ApplicationHandle) to unregister the application.
    pub async fn serve_gatt_application(
        &self, gatt_application: gatt::local::Application,
    ) -> Result<gatt::local::ApplicationHandle> {
        gatt_application.register(self.inner.clone(), self.name.clone()).await
    }

    /// Registers local GATT profiles (GATT Client).
    ///
    /// By registering this type of object
    /// an application effectively indicates support for a specific GATT profile
    /// and requests automatic connections to be established to devices
    /// supporting it.
    ///
    /// Drop the returned [ProfileHandle](gatt::local::ProfileHandle) to unregister the application.
    pub async fn register_gatt_profile(
        &self, gatt_profile: gatt::local::Profile,
    ) -> Result<gatt::local::ProfileHandle> {
        gatt_profile.register(self.inner.clone(), self.name.clone()).await
    }

    // ===========================================================================================
    // Methods
    // ===========================================================================================

    /// This removes the remote device object for the given
    /// device address.
    ///
    /// It will remove also the pairing information.
    pub async fn remove_device(&self, address: Address) -> Result<()> {
        let path = Device::dbus_path(self.name(), address)?;
        self.call_method("RemoveDevice", ((path),)).await?;
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
    /// successful this method returns the created
    /// device object.
    ///
    /// Parameters are the following:
    ///
    /// * `address` -
    ///     The Bluetooth device address of the remote
    ///     device.
    /// * `address_type` -
    ///     The Bluetooth device Address Type. This is
    ///     address type that should be used for initial
    ///     connection.
    ///
    /// This method is experimental.
    pub async fn connect_device(&self, address: Address, address_type: AddressType) -> Result<Device> {
        let mut m = PropMap::new();
        m.insert("Address".to_string(), Variant(address.to_string().box_clone()));
        match address_type {
            AddressType::LePublic | AddressType::LeRandom => {
                m.insert("AddressType".to_string(), Variant(address_type.to_string().box_clone()));
            }
            AddressType::BrEdr => (),
        }
        let (_path,): (Path,) = self.call_method("ConnectDevice", (m,)).await?;

        self.device(address)
    }
}

define_properties!(
    Adapter,
    /// Bluetooth adapter property.
    pub AdapterProperty => {

        // ===========================================================================================
        // Adapter properties
        // ===========================================================================================

        /// The Bluetooth adapter address.
        property(
            Address, Address,
            dbus: (INTERFACE, "Address", String, MANDATORY),
            get: (address, v => { v.parse()? }),
        );

        /// The Bluetooth adapter address type.
        ///
        /// For dual-mode and BR/EDR
        /// only adapter this defaults to "public". Single mode LE
        /// adapters may have either value. With privacy enabled
        /// this contains type of Identity Address and not type of
        /// address used for connection.
        property(
            AddressType, AddressType,
            dbus: (INTERFACE, "AddressType", String, MANDATORY),
            get: (address_type, v => {v.parse()?}),
        );

        ///	The Bluetooth system name (pretty hostname).
        ///
        /// This property is either a static system default
        /// or controlled by an external daemon providing
        /// access to the pretty hostname configuration.
        property(
            SystemName, String,
            dbus: (INTERFACE, "Name", String, MANDATORY),
            get: (system_name, v => {v.to_owned()}),
        );

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
        property(
            Alias, String,
            dbus: (INTERFACE, "Alias", String, MANDATORY),
            get: (alias, v => {v.to_owned()}),
            set: (set_alias, v => {v}),
        );

        /// The Bluetooth class of device.
        ///
        ///	This property represents the value that is either
        ///	automatically configured by DMI/ACPI information
        ///	or provided as static configuration.
        property(
            Class, u32,
            dbus: (INTERFACE, "Class", u32, MANDATORY),
            get: (class, v => {v.to_owned()}),
        );

        /// Switch an adapter on or off. This will also set the
        /// appropriate connectable state of the controller.
        ///
        /// The value of this property is not persistent. After
        /// restart or unplugging of the adapter it will reset
        /// back to false.
        property(
            Powered, bool,
            dbus: (INTERFACE, "Powered", bool, MANDATORY),
            get: (is_powered, v => {v.to_owned()}),
            set: (set_powered, v => {v}),
        );

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
        ///
        /// To send Bluetooth LE advertisements use the
        /// [advertise](Adapter::advertise) method instead.
        property(
            Discoverable, bool,
            dbus: (INTERFACE, "Discoverable", bool, MANDATORY),
            get: (is_discoverable, v => {v.to_owned()}),
            set: (set_discoverable, v => {v}),
        );

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
        property(
            Pairable, bool,
            dbus: (INTERFACE, "Pairable", bool, MANDATORY),
            get: (is_pairable, v => {v.to_owned()}),
            set: (set_pairable, v => {v}),
        );

        /// The pairable timeout in seconds.
        ///
        /// A value of zero
        /// means that the timeout is disabled and it will stay in
        /// pairable mode forever.
        ///
        /// The default value for pairable timeout should be
        /// disabled (value 0).
        property(
            PairableTimeout, u32,
            dbus: (INTERFACE, "PairableTimeout", u32, MANDATORY),
            get: (pairable_timeout, v => {v.to_owned()}),
            set: (set_pairable_timeout, v => {v}),
        );

        /// The discoverable timeout in seconds.
        ///
        /// A value of zero
        /// means that the timeout is disabled and it will stay in
        /// discoverable/limited mode forever.
        ///
        /// The default value for the discoverable timeout should
        /// be 180 seconds (3 minutes).
        property(
            DiscoverableTimeout, u32,
            dbus: (INTERFACE, "DiscoverableTimeout", u32, MANDATORY),
            get: (discoverable_timeout, v => {v.to_owned()}),
            set: (set_discoverable_timeout, v => {v}),
        );

        ///	Indicates that a device discovery procedure is active.
        property(
            Discovering, bool,
            dbus: (INTERFACE, "Discovering", bool, MANDATORY),
            get: (is_discovering, v => {v.to_owned()}),
        );

        /// List of 128-bit UUIDs that represents the available
        /// local services.
        property(
            Uuids, HashSet<Uuid>,
            dbus: (INTERFACE, "UUIDs", Vec<String>, OPTIONAL),
            get: (uuids, v => {
                v
                .iter()
                .map(|uuid| {
                    uuid.parse()
                        .map_err(|_| Error::new(ErrorKind::Internal(InternalErrorKind::InvalidUuid(uuid.to_string()))))
                })
                .collect::<Result<HashSet<Uuid>>>()?
            }),
        );

        /// Local Device ID information in modalias format
        /// used by the kernel and udev.
        property(
            Modalias, Modalias,
            dbus: (INTERFACE, "Modalias", String, OPTIONAL),
            get: (modalias, v => { v.parse()? }),
        );

        // ===========================================================================================
        // LE advertising manager properties
        // ===========================================================================================

        ///	Number of active advertising instances.
        property(
            ActiveAdvertisingInstances, u8,
            dbus: (adv::MANAGER_INTERFACE, "ActiveInstances", u8, MANDATORY),
            get: (active_advertising_instances, v => {v.to_owned()}),
        );

        ///	Number of available advertising instances.
        property(
            SupportedAdvertisingInstances, u8,
            dbus: (adv::MANAGER_INTERFACE, "SupportedInstances", u8, MANDATORY),
            get: (supported_advertising_instances, v => {v.to_owned()}),
        );

        /// List of supported system includes.
        property(
            SupportedAdvertisingSystemIncludes, BTreeSet<Feature>,
            dbus: (adv::MANAGER_INTERFACE, "SupportedIncludes", Vec<String>, MANDATORY),
            get: (supported_advertising_system_includes, v => {
                v.iter().filter_map(|s| s.parse().ok()).collect()
            }),
        );

        /// List of supported Secondary channels.
        ///
        /// Secondary
        /// channels can be used to advertise with the
        /// corresponding PHY.
        property(
            SupportedAdvertisingSecondaryChannels, BTreeSet<SecondaryChannel>,
            dbus: (adv::MANAGER_INTERFACE, "SupportedSecondaryChannels", Vec<String>, OPTIONAL),
            get: (supported_advertising_secondary_channels, v => {
                v.iter().filter_map(|s| s.parse().ok()).collect()
            }),
        );

        /// Enumerates Advertising-related controller capabilities
        /// useful to the client.
        property(
            SupportedAdvertisingCapabilities, Capabilities,
            dbus: (adv::MANAGER_INTERFACE, "SupportedCapabilities", HashMap<String, Variant<Box<dyn RefArg  + 'static>>>, OPTIONAL),
            get: (supported_advertising_capabilities, v => {
                Capabilities::from_dict(v)?
            }),
        );

        /// List of supported platform features.
        ///
        /// If no features
        /// are available on the platform, the SupportedFeatures
        /// array will be empty.
        property(
            SupportedAdvertisingFeatures, BTreeSet<PlatformFeature>,
            dbus: (adv::MANAGER_INTERFACE, "SupportedFeatures", Vec<String>, OPTIONAL),
            get: (supported_advertising_features, v => {
                v.iter().filter_map(|s| s.parse().ok()).collect()
            }),
        );
    }
);

/// Bluetooth adapter event.
#[cfg_attr(docsrs, doc(cfg(feature = "bluetoothd")))]
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum AdapterEvent {
    /// Bluetooth device with specified address was added.
    DeviceAdded(Address),
    /// Bluetooth device with specified address was removed.
    DeviceRemoved(Address),
    /// Bluetooth adapter property changed.
    PropertyChanged(AdapterProperty),
}

/// Transport parameter determines the type of scan.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Display, EnumString)]
pub(crate) enum DiscoveryTransport {
    /// interleaved scan
    #[strum(serialize = "auto")]
    Auto,
    /// BR/EDR inquiry
    #[strum(serialize = "bredr")]
    BrEdr,
    /// LE scan only
    #[strum(serialize = "le")]
    Le,
}

impl Default for DiscoveryTransport {
    fn default() -> Self {
        Self::Auto
    }
}

/// Bluetooth device discovery filter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DiscoveryFilter {
    ///  Filter by service UUIDs, empty means match
    ///  _any_ UUID.
    ///
    ///  When a remote device is found that advertises
    ///  any UUID from UUIDs, it will be reported if:
    ///  - pathloss and RSSI are both empty.
    ///  - only pathloss param is set, device advertise
    ///    TX power, and computed pathloss is less than
    ///    pathloss param.
    ///  - only RSSI param is set, and received RSSI is
    ///    higher than RSSI param.
    pub uuids: HashSet<Uuid>,
    /// RSSI threshold value.
    ///
    /// PropertiesChanged signals will be emitted
    /// for already existing Device objects, with
    /// updated RSSI value. If one or more discovery
    /// filters have been set, the RSSI delta-threshold,
    /// that is imposed by StartDiscovery by default,
    /// will not be applied.
    pub rssi: Option<i16>,
    /// Pathloss threshold value.
    ///
    /// PropertiesChanged signals will be emitted
    /// for already existing Device objects, with
    /// updated Pathloss value.
    pub pathloss: Option<u16>,
    /// Transport parameter determines the type of
    /// scan.
    ///
    /// Possible values:
    ///     "auto"  - interleaved scan
    ///     "bredr" - BR/EDR inquiry
    ///     "le"    - LE scan only
    ///
    /// If "le" or "bredr" Transport is requested,
    /// and the controller doesn't support it,
    /// org.bluez.Error.Failed error will be returned.
    ///
    /// If "auto" transport is requested, scan will use
    /// LE, BREDR, or both, depending on what's
    /// currently enabled on the controller.
    pub transport: DiscoveryTransport,
    /// Disables duplicate detection of advertisement data.
    ///
    /// When enabled PropertiesChanged signals will be
    /// generated for either ManufacturerData and
    /// ServiceData every time they are discovered.
    pub duplicate_data: bool,
    /// Make adapter discoverable while discovering.
    ///
    /// If the adapter is already discoverable setting
    /// this filter won't do anything.
    pub discoverable: bool,
    /// Discover devices where the pattern matches
    /// either the prefix of the address or
    /// device name which is convenient way to limited
    /// the number of device objects created during a
    /// discovery.
    ///
    /// When set disregards device discoverable flags.
    ///
    /// Note: The pattern matching is ignored if there
    /// are other client that don't set any pattern as
    /// it work as a logical OR, also setting empty
    /// string "" pattern will match any device found.
    pub pattern: Option<String>,
}

impl Default for DiscoveryFilter {
    fn default() -> Self {
        Self {
            uuids: Default::default(),
            rssi: Default::default(),
            pathloss: Default::default(),
            transport: Default::default(),
            duplicate_data: true,
            discoverable: false,
            pattern: Default::default(),
        }
    }
}

impl DiscoveryFilter {
    fn into_dict(self) -> HashMap<&'static str, Variant<Box<dyn RefArg>>> {
        let mut hm: HashMap<&'static str, Variant<Box<dyn RefArg>>> = HashMap::new();
        let Self { uuids, rssi, pathloss, transport, duplicate_data, discoverable, pattern } = self;
        hm.insert("UUIDs", Variant(Box::new(uuids.into_iter().map(|uuid| uuid.to_string()).collect::<Vec<_>>())));
        if let Some(rssi) = rssi {
            hm.insert("RSSI", Variant(Box::new(rssi)));
        }
        if let Some(pathloss) = pathloss {
            hm.insert("Pathloss", Variant(Box::new(pathloss)));
        }
        hm.insert("Transport", Variant(Box::new(transport.to_string())));
        hm.insert("DuplicateData", Variant(Box::new(duplicate_data)));
        hm.insert("Discoverable", Variant(Box::new(discoverable)));
        if let Some(pattern) = pattern {
            hm.insert("Pattern", Variant(Box::new(pattern)));
        }
        hm
    }
}
