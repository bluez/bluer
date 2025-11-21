//! Bluetooth LE advertising.

use futures::channel::oneshot;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fmt,
    sync::Arc,
    time::Duration,
};
use strum::{Display, EnumString};
use uuid::Uuid;
use zbus::{
    interface,
    zvariant::{OwnedObjectPath, Value},
};

use crate::{Adapter, Result, SessionInner, SERVICE_NAME};

pub(crate) const MANAGER_INTERFACE: &str = "org.bluez.LEAdvertisingManager1";
pub(crate) const ADVERTISEMENT_PREFIX: &str = "/org/bluez/bluer/advertisement";

/// Determines the type of advertising packet requested.
#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord, Hash, Display, EnumString, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Type {
    /// Broadcast
    #[strum(serialize = "broadcast")]
    Broadcast,
    /// Peripheral
    #[strum(serialize = "peripheral")]
    #[default]
    Peripheral,
}

/// Secondary channel for advertisement.
#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord, Hash, Display, EnumString, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum SecondaryChannel {
    /// 1M
    #[strum(serialize = "1M")]
    #[default]
    OneM,
    /// 2M
    #[strum(serialize = "2M")]
    TwoM,
    /// Coded
    #[strum(serialize = "Coded")]
    Coded,
}

/// Advertisement feature.
#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord, Hash, Display, EnumString)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum Feature {
    /// TX power.
    #[strum(serialize = "tx-power")]
    TxPower,
    /// Appearance.
    #[strum(serialize = "appearance")]
    Appearance,
    /// Local name.
    #[strum(serialize = "local-name")]
    LocalName,
}

/// LE advertising platform feature.
#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord, Hash, Display, EnumString)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum PlatformFeature {
    /// Indicates whether platform can
    /// specify TX power on each
    /// advertising instance.
    #[strum(serialize = "CanSetTxPower")]
    CanSetTxPower,
    /// Indicates whether multiple
    /// advertising will be offloaded
    /// to the controller.
    #[strum(serialize = "HardwareOffload")]
    HardwareOffload,
}

/// Advertising-related controller capabilities.
#[derive(Clone, Debug, Default, Eq, PartialEq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct Capabilities {
    /// Maximum advertising data length.
    pub max_advertisement_length: u8,
    /// Maximum advertising scan response length.
    pub max_scan_response_length: u8,
    /// Minimum advertising TX power (dBm).
    pub min_tx_power: Option<i16>,
    /// Maximum advertising TX power (dBm).
    pub max_tx_power: Option<i16>,
}

impl Capabilities {
    pub(crate) fn from_dict(dict: &HashMap<String, crate::zbus::zvariant::OwnedValue>) -> Result<Self> {
        Ok(Self {
            max_advertisement_length: crate::read_dict(dict, "MaxAdvLen")?,
            max_scan_response_length: crate::read_dict(dict, "MaxScnRspLen")?,
            min_tx_power: crate::read_dict(dict, "MinTxPower").ok(),
            max_tx_power: crate::read_dict(dict, "MaxTxPower").ok(),
        })
    }
}

/// Bluetooth LE advertisement data definition.
///
/// Specifies the Advertisement Data to be broadcast and some advertising
/// parameters.  Properties which are not present will not be included in the
/// data.  Required advertisement data types will always be included.
/// All UUIDs are 128-bit versions in the API, and 16 or 32-bit
/// versions of the same UUID will be used in the advertising data as appropriate.
///
/// Use [Adapter::advertise] to register a new advertisement.
#[derive(Clone, Debug, Default, Eq, PartialEq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
pub struct Advertisement {
    /// Determines the type of advertising packet requested.
    pub advertisement_type: Type,
    /// List of UUIDs to include in the "Service UUID" field of
    /// the Advertising Data.
    pub service_uuids: BTreeSet<Uuid>,
    /// Manufacturer Data fields to include in
    /// the Advertising Data.
    ///
    /// Keys are the Manufacturer ID
    /// to associate with the data.
    pub manufacturer_data: BTreeMap<u16, Vec<u8>>,
    /// Array of UUIDs to include in "Service Solicitation"
    /// Advertisement Data.
    pub solicit_uuids: BTreeSet<Uuid>,
    /// Service Data elements to include.
    ///
    /// The keys are the
    /// UUID to associate with the data.
    pub service_data: BTreeMap<Uuid, Vec<u8>>,
    /// Advertising Type to include in the Advertising
    /// Data.
    ///
    /// Key is the advertising type and value is the
    /// data as byte array.
    ///
    /// Note: Types already handled by other properties shall
    /// not be used.
    pub advertising_data: BTreeMap<u8, Vec<u8>>,
    /// Advertise as general discoverable.
    ///
    /// When present this
    /// will override adapter Discoverable property.
    ///
    /// Note: This property shall not be set when Type is set
    /// to broadcast. Additionally, Types that are official
    /// Bluetooth assigned numbers cannot be used. So for
    /// example the Type value of 0x0a cannot be used because
    /// it is assigned as the TX Power Level Type. But,
    /// currently the Type 0x0c is unassigned and can be used.
    pub discoverable: Option<bool>,
    /// The discoverable timeout in seconds.
    ///
    /// A value of zero
    /// means that the timeout is disabled and it will stay in
    /// discoverable/limited mode forever.
    ///
    /// Note: This property shall not be set when Type is set
    /// to broadcast.
    pub discoverable_timeout: Option<Duration>,
    /// List of system features to be included in the advertising
    /// packet.
    pub system_includes: BTreeSet<Feature>,
    /// Local name to be used in the advertising report.
    ///
    /// If the
    /// string is too big to fit into the packet it will be
    /// truncated.
    pub local_name: Option<String>,
    /// Appearance to be used in the advertising report.
    pub appearance: Option<u16>,
    /// Duration of the advertisement in seconds.
    ///
    /// If there are
    /// other applications advertising no duration is set the
    /// default is 2 seconds.
    pub duration: Option<Duration>,
    /// Timeout of the advertisement in seconds.
    ///
    /// This defines
    /// the lifetime of the advertisement.
    pub timeout: Option<Duration>,
    /// Secondary channel to be used.
    ///
    /// Primary channel is
    /// always set to "1M" except when "Coded" is set.
    pub secondary_channel: Option<SecondaryChannel>,
    /// Minimum advertising interval to be used by the
    /// advertising set, in milliseconds.
    ///
    /// Acceptable values
    /// are in the range [20ms, 10,485s]. If the provided
    /// MinInterval is larger than the provided MaxInterval,
    /// the registration will return failure.
    pub min_interval: Option<Duration>,
    /// Maximum advertising interval to be used by the
    /// advertising set, in milliseconds.
    ///
    /// Acceptable values
    /// are in the range [20ms, 10,485s]. If the provided
    /// MinInterval is larger than the provided MaxInterval,
    /// the registration will return failure.
    pub max_interval: Option<Duration>,
    /// Requested transmission power of this advertising set.
    ///
    /// The provided value is used only if the "CanSetTxPower"
    /// feature is enabled on the Advertising Manager. The
    /// provided value must be in range [-127 to +20], where
    /// units are in dBm.
    pub tx_power: Option<i16>,
    #[doc(hidden)]
    pub _non_exhaustive: (),
}

#[interface(name = "org.bluez.LEAdvertisement1")]
impl Advertisement {
    /// Type.
    #[zbus(property)]
    fn type_(&self) -> String {
        self.advertisement_type.to_string()
    }

    /// Service UUIDs.
    #[zbus(property, name = "ServiceUUIDs")]
    fn service_uuids(&self) -> Vec<String> {
        self.service_uuids.iter().map(|uuid| uuid.to_string()).collect()
    }

    /// Manufacturer data.
    #[zbus(property, name = "ManufacturerData")]
    fn manufacturer_data(&self) -> HashMap<u16, Value<'_>> {
        self.manufacturer_data.iter().map(|(k, v)| (*k, Value::from(v.clone()))).collect()
    }

    /// Solicit UUIDs.
    #[zbus(property, name = "SolicitUUIDs")]
    fn solicit_uuids(&self) -> Vec<String> {
        self.solicit_uuids.iter().map(|uuid| uuid.to_string()).collect()
    }

    /// Service data.
    #[zbus(property, name = "ServiceData")]
    fn service_data(&self) -> HashMap<String, Value<'_>> {
        self.service_data.iter().map(|(k, v)| (k.to_string(), Value::from(v.clone()))).collect()
    }

    /// Data.
    #[zbus(property, name = "Data")]
    fn data(&self) -> HashMap<u8, Value<'_>> {
        self.advertising_data.iter().map(|(k, v)| (*k, Value::from(v.clone()))).collect()
    }

    /// Discoverable.
    #[zbus(property)]
    fn discoverable(&self) -> std::result::Result<bool, zbus::fdo::Error> {
        self.discoverable.ok_or_else(|| zbus::fdo::Error::UnknownProperty("Discoverable".into()))
    }

    /// Discoverable timeout.
    #[zbus(property, name = "DiscoverableTimeout")]
    fn discoverable_timeout(&self) -> std::result::Result<u16, zbus::fdo::Error> {
        self.discoverable_timeout
            .map(|t| t.as_secs().min(u16::MAX as _) as u16)
            .ok_or_else(|| zbus::fdo::Error::UnknownProperty("DiscoverableTimeout".into()))
    }

    /// Includes.
    #[zbus(property, name = "Includes")]
    fn includes(&self) -> Vec<String> {
        self.system_includes.iter().map(|v| v.to_string()).collect()
    }

    /// Local name.
    #[zbus(property, name = "LocalName")]
    fn local_name(&self) -> std::result::Result<String, zbus::fdo::Error> {
        self.local_name.clone().ok_or_else(|| zbus::fdo::Error::UnknownProperty("LocalName".into()))
    }

    /// Appearance.
    #[zbus(property)]
    fn appearance(&self) -> std::result::Result<u16, zbus::fdo::Error> {
        self.appearance.ok_or_else(|| zbus::fdo::Error::UnknownProperty("Appearance".into()))
    }

    /// Duration.
    #[zbus(property)]
    fn duration(&self) -> std::result::Result<u16, zbus::fdo::Error> {
        self.duration
            .map(|t| t.as_secs().min(u16::MAX as _) as u16)
            .ok_or_else(|| zbus::fdo::Error::UnknownProperty("Duration".into()))
    }

    /// Timeout.
    #[zbus(property)]
    fn timeout(&self) -> std::result::Result<u16, zbus::fdo::Error> {
        self.timeout
            .map(|t| t.as_secs().min(u16::MAX as _) as u16)
            .ok_or_else(|| zbus::fdo::Error::UnknownProperty("Timeout".into()))
    }

    /// Secondary channel.
    #[zbus(property, name = "SecondaryChannel")]
    fn secondary_channel(&self) -> std::result::Result<String, zbus::fdo::Error> {
        self.secondary_channel
            .map(|v| v.to_string())
            .ok_or_else(|| zbus::fdo::Error::UnknownProperty("SecondaryChannel".into()))
    }

    /// Min interval.
    #[zbus(property, name = "MinInterval")]
    fn min_interval(&self) -> std::result::Result<u32, zbus::fdo::Error> {
        self.min_interval
            .map(|t| t.as_millis().min(u32::MAX as _) as u32)
            .ok_or_else(|| zbus::fdo::Error::UnknownProperty("MinInterval".into()))
    }

    /// Max interval.
    #[zbus(property, name = "MaxInterval")]
    fn max_interval(&self) -> std::result::Result<u32, zbus::fdo::Error> {
        self.max_interval
            .map(|t| t.as_millis().min(u32::MAX as _) as u32)
            .ok_or_else(|| zbus::fdo::Error::UnknownProperty("MaxInterval".into()))
    }

    /// Tx power.
    #[zbus(property, name = "TxPower")]
    fn tx_power(&self) -> std::result::Result<i16, zbus::fdo::Error> {
        self.tx_power.ok_or_else(|| zbus::fdo::Error::UnknownProperty("TxPower".into()))
    }
}

impl Advertisement {
    pub(crate) async fn register(
        self, inner: Arc<SessionInner>, adapter_name: Arc<String>,
    ) -> Result<AdvertisementHandle> {
        let path = format!("{}/{}", ADVERTISEMENT_PREFIX, Uuid::new_v4().as_simple());
        let path = OwnedObjectPath::try_from(path).unwrap();
        log::trace!("Publishing advertisement at {}", &path);

        let _ = inner.connection.object_server().at(&path, self).await?;

        log::trace!("Registering advertisement at {}", &path);
        let proxy = zbus::Proxy::new(
            &inner.connection,
            SERVICE_NAME,
            Adapter::dbus_path(&adapter_name)?,
            MANAGER_INTERFACE,
        )
        .await?;
        let () = proxy.call("RegisterAdvertisement", &(&path, HashMap::<String, Value>::new())).await?;

        let (drop_tx, drop_rx) = oneshot::channel();
        let unreg_path = path.clone();
        let connection = inner.connection.clone();
        let adapter_name = adapter_name.clone();
        tokio::spawn(async move {
            let _ = drop_rx.await;

            log::trace!("Unregistering advertisement at {}", &unreg_path);
            if let Ok(adapter_path) = Adapter::dbus_path(&adapter_name) {
                if let Ok(proxy) =
                    zbus::Proxy::new(&connection, SERVICE_NAME, adapter_path, MANAGER_INTERFACE).await
                {
                    let _: std::result::Result<(), zbus::Error> =
                        proxy.call("UnregisterAdvertisement", &(&unreg_path,)).await;
                }
            }

            log::trace!("Unpublishing advertisement at {}", &unreg_path);
            let _ = connection.object_server().remove::<Self, _>(&unreg_path).await;
        });

        Ok(AdvertisementHandle { path, _drop_tx: drop_tx })
    }
}

/// Handle to active Bluetooth LE advertisement.
///
/// Drop to unregister advertisement.
#[must_use = "AdvertisementHandle must be held for advertisement to be broadcasted"]
pub struct AdvertisementHandle {
    path: OwnedObjectPath,
    _drop_tx: oneshot::Sender<()>,
}

impl Drop for AdvertisementHandle {
    fn drop(&mut self) {
        // required for drop order
    }
}

impl fmt::Debug for AdvertisementHandle {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "AdvertisementHandle {{ {} }}", &self.path)
    }
}
