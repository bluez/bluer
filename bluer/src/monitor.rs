//! Bluetooth advertisement monitor.
//!
//! This API allows an client to specify a job of monitoring advertisements by
//! exposing advertisement monitors with filtering conditions, thresholds of RSSI and timers
//! of RSSI thresholds.

use zbus::{
    interface,
    zvariant::{ObjectPath, OwnedObjectPath, Value},
};
use futures::{Stream, StreamExt};
use std::{
    fmt,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::Duration,
    collections::HashMap,
};
use strum::{Display, EnumString};
use tokio::sync::{mpsc, oneshot, Mutex as AsyncMutex};
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

use crate::{
    Address, Device, Error, ErrorKind, Result, SessionInner, SERVICE_NAME,
};

pub(crate) const INTERFACE: &str = "org.bluez.AdvertisementMonitor1";
pub(crate) const MANAGER_INTERFACE: &str = "org.bluez.AdvertisementMonitorManager1";
pub(crate) const MANAGER_PATH: &str = "/org/bluez";
pub(crate) const MONITOR_PREFIX: &str = "/org/bluez/bluer/monitor";

/// Determines the type of advertisement monitor.
#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord, Hash, Display, EnumString)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum Type {
    /// Patterns with logic OR applied.
    #[strum(serialize = "or_patterns")]
    OrPatterns,
}

impl Default for Type {
    fn default() -> Self {
        Self::OrPatterns
    }
}

/// Common advertising data types for [`Pattern::data_type`].
///
/// See [the GATT specification](https://www.bluetooth.com/specifications/assigned-numbers/generic-access-profile/)
/// for a complete list.
pub mod data_type {
    /// Flags: Contains important settings for the device such as BR/EDR and LE modes.
    pub const FLAGS: u8 = 0x01;

    /// Incomplete List of 16-bit Service Class UUIDs:
    /// Contains a list of 16-bit UUIDs as defined by the Bluetooth SIG that the device advertises, but the list is not complete.
    pub const INCOMPLETE_LIST_16_BIT_SERVICE_CLASS_UUIDS: u8 = 0x02;

    /// Complete List of 16-bit Service Class UUIDs:
    /// Contains a complete list of 16-bit UUIDs as defined by the Bluetooth SIG that the device advertises.
    pub const COMPLETE_LIST_16_BIT_SERVICE_CLASS_UUIDS: u8 = 0x03;

    /// Incomplete List of 32-bit Service Class UUIDs:
    /// Contains a list of 32-bit UUIDs as defined by the Bluetooth SIG that the device advertises, but the list is not complete.
    pub const INCOMPLETE_LIST_32_BIT_SERVICE_CLASS_UUIDS: u8 = 0x04;

    /// Complete List of 32-bit Service Class UUIDs:
    /// Contains a complete list of 32-bit UUIDs as defined by the Bluetooth SIG that the device advertises.
    pub const COMPLETE_LIST_32_BIT_SERVICE_CLASS_UUIDS: u8 = 0x05;

    /// Incomplete List of 128-bit Service Class UUIDs:
    /// Contains a list of 128-bit UUIDs that the device advertises, but the list is not complete.
    pub const INCOMPLETE_LIST_128_BIT_SERVICE_CLASS_UUIDS: u8 = 0x06;

    /// Complete List of 128-bit Service Class UUIDs:
    /// Contains a complete list of 128-bit UUIDs that the device advertises.
    pub const COMPLETE_LIST_128_BIT_SERVICE_CLASS_UUIDS: u8 = 0x07;

    /// Shortened Local Name: Contains a shortened version of the local device name.
    pub const SHORTENED_LOCAL_NAME: u8 = 0x08;

    /// Complete Local Name: Contains the complete local device name.
    pub const COMPLETE_LOCAL_NAME: u8 = 0x09;

    /// TX Power Level: Contains the device's transmit power level.
    pub const TX_POWER_LEVEL: u8 = 0x0A;

    /// Manufacturer Specific Data: Contains data specific to the manufacturer.
    pub const MANUFACTURER_SPECIFIC_DATA: u8 = 0xFF;
}

/// An advertisement data pattern, used to filter devices in the advertisement monitor.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Pattern {
    /// Advertising data type to match.
    ///
    /// See [data_type] for common values.
    pub data_type: u8,
    /// The index in an AD data field where the search should start.
    ///
    /// The beginning of an AD data field is index 0.
    pub start_position: u8,
    /// The value of the pattern.
    ///
    /// The maximum length of the bytes is 31.
    pub content: Vec<u8>,
}

impl Pattern {
    /// Creates a new advertisement data pattern.
    ///
    /// See the field documentation for more information about the arguments.
    pub fn new(data_type: u8, start_position: u8, content: &[u8]) -> Self {
        Self { data_type, start_position, content: content.to_vec() }
    }
}

/// Grouping rules on how to propagate the received
/// advertisement packets to the client.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum RssiSamplingPeriod {
    /// All advertisement packets from in-range devices
    /// would be propagated.
    All,
    /// Only the first advertisement packet of in-range
    /// devices would be propagated. If the device
    /// becomes lost, then the first packet when it is
    /// found again will also be propagated.
    First,
    /// Advertisement packets would be grouped into
    /// the specified time period rounded to 100ms.
    /// Packets in the same group will only be reported once,
    /// with the RSSI value being averaged out.
    Period(Duration),
}

impl RssiSamplingPeriod {
    fn to_value(self) -> u16 {
        match self {
            Self::All => 0,
            Self::First => 255,
            Self::Period(period) => (period.as_millis() / 100).clamp(1, 254) as u16,
        }
    }
}

/// Advertisement monitor specification.
///
/// Specifies an advertisement monitor target.
///
/// Use [`MonitorManager::register`] to add a monitor target.
#[derive(Default, Clone)]
pub struct Monitor {
    /// The type of the monitor.
    pub monitor_type: Type,

    /// Used in conjunction with RSSILowTimeout to determine
    /// whether a device becomes out-of-range.
    ///
    /// Valid range is -127 to 20 (dBm).
    pub rssi_low_threshold: Option<i16>,

    /// Used in conjunction with RSSIHighTimeout to determine
    /// whether a device becomes in-range.
    ///
    /// Valid range is -127 to 20 (dBm).
    pub rssi_high_threshold: Option<i16>,

    /// The time it takes to consider a device as out-of-range.
    ///
    /// If this many seconds elapses without receiving any
    /// signal at least as strong as RSSILowThreshold, a
    /// currently in-range device will be considered as
    /// out-of-range (lost).
    ///
    /// Valid range is 1 to 300 (seconds).
    pub rssi_low_timeout: Option<Duration>,

    /// The time it takes to consider a device as in-range.
    ///
    /// If this many seconds elapses while we continuously
    /// receive signals at least as strong as RSSIHighThreshold,
    /// a currently out-of-range device will be considered as
    /// in-range (found).
    ///
    /// Valid range is 1 to 300 (seconds).
    pub rssi_high_timeout: Option<Duration>,

    /// Grouping rules on how to propagate the received
    /// advertisement packets to the client.
    pub rssi_sampling_period: Option<RssiSamplingPeriod>,

    /// Patterns to match.
    ///
    /// Required if [`monitor_type`](Self::monitor_type) is
    /// [`Type::OrPatterns`].
    pub patterns: Option<Vec<Pattern>>,

    #[doc(hidden)]
    pub _non_exhaustive: (),
}

/// Information identifying a found or lost device.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub struct DeviceId {
    /// Bluetooth adapter that found or lost the device.
    pub adapter: String,
    /// Device address.
    pub device: Address,
}

/// An advertisement monitor event.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum MonitorEvent {
    /// This event notifies the client of finding the
    /// targeted device.
    ///
    /// Once receiving the event, the client
    /// should start to monitor the corresponding device to
    /// retrieve the changes on RSSI and advertisement content.
    DeviceFound(DeviceId),

    /// This event notifies the client of losing the
    /// targeted device.
    ///
    /// Once receiving this event, the client
    /// should stop monitoring the corresponding device.
    DeviceLost(DeviceId),
}

pub(crate) struct RegisteredMonitor {
    am: Monitor,
    activate_tx: mpsc::Sender<()>,
    release_tx: mpsc::Sender<()>,
    event_tx: AsyncMutex<Option<mpsc::Sender<MonitorEvent>>>,
}

impl RegisteredMonitor {
    fn parse_device_path(device: &ObjectPath) -> zbus::fdo::Result<(String, Address)> {
        match Device::parse_dbus_path(device) {
            Some((adapter, addr)) => Ok((adapter.to_string(), addr)),
            None => {
                log::error!("Cannot parse device path {}", &device);
                Err(zbus::fdo::Error::InvalidArgs("cannot parse device path".into()))
            }
        }
    }
}

#[interface(name = "org.bluez.AdvertisementMonitor1")]
impl RegisteredMonitor {
    async fn release(&self) {
        *self.event_tx.lock().await = None;
        let _ = self.release_tx.send(()).await;
    }

    async fn activate(&self) {
        let _ = self.activate_tx.send(()).await;
    }

    async fn device_found(&self, device: ObjectPath<'_>) -> zbus::fdo::Result<()> {
        let (adapter, device) = Self::parse_device_path(&device)?;
        if let Some(event_tx) = self.event_tx.lock().await.as_ref() {
            let _ = event_tx.send(MonitorEvent::DeviceFound(DeviceId { adapter, device })).await;
        }
        Ok(())
    }

    async fn device_lost(&self, device: ObjectPath<'_>) -> zbus::fdo::Result<()> {
        let (adapter, device) = Self::parse_device_path(&device)?;
        if let Some(event_tx) = self.event_tx.lock().await.as_ref() {
            let _ = event_tx.send(MonitorEvent::DeviceLost(DeviceId { adapter, device })).await;
        }
        Ok(())
    }

    #[zbus(property)]
    fn type_(&self) -> String {
        self.am.monitor_type.to_string()
    }

    #[zbus(property, name = "RSSILowThreshold")]
    fn rssi_low_threshold(&self) -> std::result::Result<i16, zbus::fdo::Error> {
        self.am.rssi_low_threshold.ok_or_else(|| zbus::fdo::Error::UnknownProperty("RSSILowThreshold".into()))
    }

    #[zbus(property, name = "RSSIHighThreshold")]
    fn rssi_high_threshold(&self) -> std::result::Result<i16, zbus::fdo::Error> {
        self.am.rssi_high_threshold.ok_or_else(|| zbus::fdo::Error::UnknownProperty("RSSIHighThreshold".into()))
    }

    #[zbus(property, name = "RSSILowTimeout")]
    fn rssi_low_timeout(&self) -> std::result::Result<u16, zbus::fdo::Error> {
        self.am.rssi_low_timeout.map(|t| t.as_secs().clamp(1, 300) as u16).ok_or_else(|| zbus::fdo::Error::UnknownProperty("RSSILowTimeout".into()))
    }

    #[zbus(property, name = "RSSIHighTimeout")]
    fn rssi_high_timeout(&self) -> std::result::Result<u16, zbus::fdo::Error> {
        self.am.rssi_high_timeout.map(|t| t.as_secs().clamp(1, 300) as u16).ok_or_else(|| zbus::fdo::Error::UnknownProperty("RSSIHighTimeout".into()))
    }

    #[zbus(property, name = "RSSISamplingPeriod")]
    fn rssi_sampling_period(&self) -> std::result::Result<u16, zbus::fdo::Error> {
        self.am.rssi_sampling_period.map(|v| v.to_value()).ok_or_else(|| zbus::fdo::Error::UnknownProperty("RSSISamplingPeriod".into()))
    }

    #[zbus(property)]
    fn patterns(&self) -> std::result::Result<Vec<(u8, u8, Vec<u8>)>, zbus::fdo::Error> {
        self.am.patterns.as_ref().map(|patterns| {
            patterns
                .iter()
                .map(|p| (p.start_position, p.data_type, p.content.clone()))
                .collect()
        }).ok_or_else(|| zbus::fdo::Error::UnknownProperty("Patterns".into()))
    }
}

struct MonitorApplication {
    monitors: Arc<Mutex<HashMap<OwnedObjectPath, Monitor>>>,
}

#[interface(name = "org.freedesktop.DBus.ObjectManager")]
impl MonitorApplication {
    fn get_managed_objects(&self) -> HashMap<OwnedObjectPath, HashMap<String, HashMap<String, Value<'_>>>> {
        let monitors = self.monitors.lock().unwrap();
        let mut managed_objects = HashMap::new();
        for (path, monitor) in monitors.iter() {
            let mut interfaces = HashMap::new();
            let mut props = HashMap::new();
            
            props.insert("Type".to_string(), Value::from(monitor.monitor_type.to_string()));
            if let Some(v) = monitor.rssi_low_threshold {
                props.insert("RSSILowThreshold".to_string(), Value::from(v));
            }
            if let Some(v) = monitor.rssi_high_threshold {
                props.insert("RSSIHighThreshold".to_string(), Value::from(v));
            }
            if let Some(v) = monitor.rssi_low_timeout {
                props.insert("RSSILowTimeout".to_string(), Value::from(v.as_secs().clamp(1, 300) as u16));
            }
            if let Some(v) = monitor.rssi_high_timeout {
                props.insert("RSSIHighTimeout".to_string(), Value::from(v.as_secs().clamp(1, 300) as u16));
            }
            if let Some(v) = monitor.rssi_sampling_period {
                props.insert("RSSISamplingPeriod".to_string(), Value::from(v.to_value()));
            }
            if let Some(patterns) = &monitor.patterns {
                let p: Vec<(u8, u8, Vec<u8>)> = patterns.iter().map(|p| (p.start_position, p.data_type, p.content.clone())).collect();
                props.insert("Patterns".to_string(), Value::from(p));
            }

            interfaces.insert(INTERFACE.to_string(), props);
            managed_objects.insert(path.clone(), interfaces);
        }
        managed_objects
    }
}

/// A registered advertisement monitor target.
///
/// Use this to receive a stream of [advertisement monitor events](MonitorEvent)
/// for the registered monitor.
///
/// While a [`MonitorHandle`] is being held, its events *must* be consumed regularly.
/// Otherwise it will use an unbounded amount of memory for buffering the unconsumed events.
///
/// Drop to unregister the advertisement monitor target.
#[must_use = "the MonitorHandle must be held for the monitor to be active and its events must be consumed regularly"]
pub struct MonitorHandle {
    name: OwnedObjectPath,
    event_rx: ReceiverStream<MonitorEvent>,
    _drop_tx: oneshot::Sender<()>,
}

impl fmt::Debug for MonitorHandle {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "MonitorHandle {{ {} }}", &self.name)
    }
}

impl Stream for MonitorHandle {
    type Item = MonitorEvent;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        Pin::into_inner(self).event_rx.poll_next_unpin(cx)
    }
}

impl Drop for MonitorHandle {
    fn drop(&mut self) {
        // required for drop order
    }
}

/// Advertisement monitor manager.
///
/// Use [`Adapter::monitor`](crate::adapter::Adapter::monitor) to obtain an instance.
///
/// Once a monitoring job is activated by BlueZ, the client can expect to get
/// notified on the targeted advertisements no matter if there is an ongoing
/// discovery session.
///
/// Use this to target advertisements and drop it to stop monitoring advertisements.
pub struct MonitorManager {
    inner: Arc<SessionInner>,
    root: OwnedObjectPath,
    monitors: Arc<Mutex<HashMap<OwnedObjectPath, Monitor>>>,
    _drop_tx: oneshot::Sender<()>,
}

impl fmt::Debug for MonitorManager {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("MonitorManager").finish()
    }
}

impl MonitorManager {
    pub(crate) async fn new(inner: Arc<SessionInner>, adapter_name: &str) -> Result<Self> {
        let manager_path = format!("{}/{}", MANAGER_PATH, adapter_name);
        let root = format!("{}/{}", MONITOR_PREFIX, Uuid::new_v4().as_simple());
        let root = OwnedObjectPath::try_from(root).unwrap();

        log::trace!("Publishing advertisement monitor root at {}", &root);

        let monitors = Arc::new(Mutex::new(HashMap::new()));
        let app = MonitorApplication { monitors: monitors.clone() };
        let _ = inner.connection.object_server().at(&root, app).await?;

        log::trace!("Registering advertisement monitor root at {}", &root);
        let proxy = zbus::Proxy::new(&inner.connection, SERVICE_NAME, manager_path, MANAGER_INTERFACE).await?;
        let () = proxy.call("RegisterMonitor", &(&root,)).await?;

        let (_drop_tx, drop_rx) = oneshot::channel();
        let unreg_root = root.clone();
        let connection = inner.connection.clone();
        let proxy = proxy.clone();
        tokio::spawn(async move {
            let _ = drop_rx.await;

            log::trace!("Unregistering advertisement monitor root at {}", &unreg_root);
            let _: std::result::Result<(), zbus::Error> =
                proxy.call("UnregisterMonitor", &(&unreg_root,)).await;

            log::trace!("Unpublishing advertisement monitor root at {}", &unreg_root);
            let _ = connection.object_server().remove::<MonitorApplication, _>(&unreg_root).await;
        });

        Ok(Self { inner, root, monitors, _drop_tx })
    }

    /// Registers an advertisement monitor target.
    ///
    /// Returns a handle to receive events.
    pub async fn register(&self, advertisement_monitor: Monitor) -> Result<MonitorHandle> {
        let name = format!("{}/{}", &self.root, Uuid::new_v4().as_simple());
        let name = OwnedObjectPath::try_from(name).unwrap();

        log::trace!("Publishing advertisement monitor target at {}", &name);

        let (activate_tx, mut activate_rx) = mpsc::channel(1);
        let (release_tx, mut release_rx) = mpsc::channel(1);
        let (event_tx, event_rx) = mpsc::channel(1024);
        let (_drop_tx, drop_rx) = oneshot::channel();

        let reg = RegisteredMonitor {
            am: advertisement_monitor.clone(),
            activate_tx,
            release_tx,
            event_tx: AsyncMutex::new(Some(event_tx)),
        };

        let _ = self.inner.connection.object_server().at(&name, reg).await?;
        
        {
            let mut monitors = self.monitors.lock().unwrap();
            monitors.insert(name.clone(), advertisement_monitor);
        }

        let inner = self.inner.clone();
        let unreg_name = name.clone();
        let monitors = self.monitors.clone();
        tokio::spawn(async move {
            let _ = drop_rx.await;

            log::trace!("Unpublishing advertisement monitor target at {}", &unreg_name);
            let _ = inner.connection.object_server().remove::<RegisteredMonitor, _>(&unreg_name).await;
            
            let mut monitors = monitors.lock().unwrap();
            monitors.remove(&unreg_name);
        });

        tokio::select! {
            biased;
            _ = release_rx.recv() => return Err(Error::new(ErrorKind::AdvertisementMonitorRejected)),
            res = activate_rx.recv() => {
                if res.is_none() {
                    return Err(Error::new(ErrorKind::AdvertisementMonitorRejected))
                }
            },
        }

        Ok(MonitorHandle { name, event_rx: event_rx.into(), _drop_tx })
    }
}

impl Drop for MonitorManager {
    fn drop(&mut self) {
        // required for drop order
    }
}
