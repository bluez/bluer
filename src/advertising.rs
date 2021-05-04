use dbus::{
    arg::{PropMap, RefArg, Variant},
    nonblock::Proxy,
};
use dbus_crossroads::{Crossroads, IfaceBuilder, IfaceToken};
use futures::channel::oneshot;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fmt,
    sync::Arc,
    time::Duration,
};
use strum::{Display, EnumString};
use uuid::Uuid;

use crate::{read_dict, Adapter, Result, SessionInner, SERVICE_NAME, TIMEOUT};

pub(crate) const MANAGER_INTERFACE: &str = "org.bluez.LEAdvertisingManager1";
pub(crate) const ADVERTISEMENT_INTERFACE: &str = "org.bluez.LEAdvertisement1";
pub(crate) const ADVERTISEMENT_PREFIX: &str = "/io/crates/tokio_bluez/";

/// Determines the type of advertising packet requested.
#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord, Hash, Display, EnumString)]
pub enum LeAdvertisementType {
    /// Broadcast
    #[strum(serialize = "broadcast")]
    Broadcast,
    /// Peripheral
    #[strum(serialize = "peripheral")]
    Peripheral,
}

impl Default for LeAdvertisementType {
    fn default() -> Self {
        Self::Broadcast
    }
}

/// Secondary channel to be used.
#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord, Hash, Display, EnumString)]
pub enum LeAdvertisementSecondaryChannel {
    /// 1M
    #[strum(serialize = "1M")]
    OneM,
    /// 2M
    #[strum(serialize = "2M")]
    TwoM,
    /// Coded
    #[strum(serialize = "Coded")]
    Coded,
}

impl Default for LeAdvertisementSecondaryChannel {
    fn default() -> Self {
        Self::OneM
    }
}

/// Advertisement feature.
#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord, Hash, Display, EnumString)]
pub enum LeAdvertisementFeature {
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
pub enum LeAdvertisingFeature {
    /// Indicates whether platform can
    /// specify tx power on each
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
pub struct LeAdvertisingCapabilities {
    /// Maximum advertising data length.
    pub max_advertisement_length: u8,
    /// Maximum advertising scan response length.
    pub max_scan_response_length: u8,
    /// Minimum advertising TX power (dBm).
    pub min_tx_power: i16,
    /// Maximum advertising TX power (dBm).
    pub max_tx_power: i16,
}

impl LeAdvertisingCapabilities {
    pub(crate) fn from_dict(
        dict: &HashMap<String, Variant<Box<dyn RefArg + 'static>>>,
    ) -> Result<Self> {
        Ok(Self {
            max_advertisement_length: *read_dict(&dict, "MaxAdvLen")?,
            max_scan_response_length: *read_dict(&dict, "MaxScnRspLen")?,
            min_tx_power: *read_dict(&dict, "MinTxPower")?,
            max_tx_power: *read_dict(&dict, "MaxTxPower")?,
        })
    }
}

/// Bluetooth LE advertisement data.
///
/// Specifies the Advertisement Data to be broadcast and some advertising
/// parameters.  Properties which are not present will not be included in the
/// data.  Required advertisement data types will always be included.
/// All UUIDs are 128-bit versions in the API, and 16 or 32-bit
/// versions of the same UUID will be used in the advertising data as appropriate.
#[derive(Clone, Debug, Default, Eq, PartialEq, PartialOrd, Ord, Hash)]
pub struct LeAdvertisement {
    /// Determines the type of advertising packet requested.
    pub advertisement_type: LeAdvertisementType,
    /// List of UUIDs to include in the "Service UUID" field of
    /// the Advertising Data.
    pub service_uuids: BTreeSet<Uuid>,
    /// Manufactuer Data fields to include in
    ///	the Advertising Data.
    ///
    /// Keys are the Manufacturer ID
    ///	to associate with the data.
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
    pub advertisting_data: BTreeMap<u8, Vec<u8>>,
    /// Advertise as general discoverable.
    ///
    /// When present this
    /// will override adapter Discoverable property.
    ///
    /// Note: This property shall not be set when Type is set
    /// to broadcast.
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
    pub system_includes: BTreeSet<LeAdvertisementFeature>,
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
    pub secondary_channel: Option<LeAdvertisementSecondaryChannel>,
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
}

impl LeAdvertisement {
    pub(crate) fn register_interface(cr: &mut Crossroads) -> IfaceToken<Self> {
        cr.register(ADVERTISEMENT_INTERFACE, |b: &mut IfaceBuilder<Self>| {
            b.property("Type").get(|_, la| {
                println!("Type queried");
                Ok(la.advertisement_type.to_string())
            });
        })
    }

    pub(crate) async fn register(
        self,
        inner: Arc<SessionInner>,
        adapter_name: Arc<String>,
    ) -> Result<LeAdvertisementHandle> {
        let name = dbus::Path::new(format!(
            "{}{}",
            ADVERTISEMENT_PREFIX,
            Uuid::new_v4().to_simple()
        ))
        .unwrap();
        dbg!(&name);

        {
            let mut cr = inner.crossroads.lock().await;
            cr.insert(name.clone(), &[inner.le_advertisment_token], self);
        }

        let proxy = Proxy::new(
            SERVICE_NAME,
            Adapter::dbus_path(&*adapter_name)?,
            TIMEOUT,
            inner.connection.clone(),
        );
        println!("registering");
        proxy
            .method_call(
                MANAGER_INTERFACE,
                "RegisterAdvertisement",
                (name.clone(), PropMap::new()),
            )
            .await?;
        println!("done");

        let (drop_tx, drop_rx) = oneshot::channel();
        let unreg_name = name.clone();
        tokio::spawn(async move {
            let _ = drop_rx.await;
            let _: std::result::Result<(), dbus::Error> = proxy
                .method_call(
                    MANAGER_INTERFACE,
                    "UnregisterAdvertisement",
                    (unreg_name.clone(),),
                )
                .await;

            let mut cr = inner.crossroads.lock().await;
            let _: Option<Self> = cr.remove(&unreg_name);
        });

        Ok(LeAdvertisementHandle {
            name,
            _drop_tx: drop_tx,
        })
    }
}

pub struct LeAdvertisementHandle {
    name: dbus::Path<'static>,
    _drop_tx: oneshot::Sender<()>,
}

impl Drop for LeAdvertisementHandle {
    fn drop(&mut self) {
        // required for drop order
    }
}

impl fmt::Debug for LeAdvertisementHandle {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "LeAdvertisementHandle {{ {} }}", &self.name)
    }
}
