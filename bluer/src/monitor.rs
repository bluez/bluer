//! Bluetooth advertisement monitor.
//!
//! This API allows an client to specify a job of monitoring advertisements by
//! exposing advertisement monitors with filtering conditions, thresholds of RSSI and timers
//! of RSSI thresholds.

use dbus::nonblock::Proxy;
use dbus_crossroads::{Crossroads, IfaceBuilder, IfaceToken};
use futures::{Stream, StreamExt};
use std::{
    fmt,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};
use strum::{Display, EnumString};
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

use crate::{
    method_call, Address, DbusResult, Device, Error, ErrorKind, Result, SessionInner, SERVICE_NAME, TIMEOUT,
};

pub(crate) const INTERFACE: &str = "org.bluez.AdvertisementMonitor1";
pub(crate) const MANAGER_INTERFACE: &str = "org.bluez.AdvertisementMonitorManager1";
pub(crate) const MANAGER_PATH: &str = "/org/bluez";
pub(crate) const MONITOR_PREFIX: &str = publish_path!("monitor");

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
#[derive(Default)]
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
    event_tx: Mutex<Option<mpsc::Sender<MonitorEvent>>>,
}

impl RegisteredMonitor {
    fn parse_device_path(device: &dbus::Path<'static>) -> DbusResult<(String, Address)> {
        match Device::parse_dbus_path(device) {
            Some((adapter, addr)) => Ok((adapter.to_string(), addr)),
            None => {
                log::error!("Cannot parse device path {}", &device);
                Err(dbus::MethodErr::invalid_arg("cannot parse device path"))
            }
        }
    }

    pub(crate) fn register_interface(cr: &mut Crossroads) -> IfaceToken<Arc<RegisteredMonitor>> {
        cr.register(INTERFACE, |ib: &mut IfaceBuilder<Arc<RegisteredMonitor>>| {
            ib.method_with_cr_async("Release", (), (), |ctx, cr, ()| {
                method_call(ctx, cr, |reg: Arc<RegisteredMonitor>| async move {
                    *reg.event_tx.lock().await = None;
                    let _ = reg.release_tx.send(()).await;
                    Ok(())
                })
            });

            ib.method_with_cr_async("Activate", (), (), |ctx, cr, ()| {
                method_call(ctx, cr, |reg: Arc<RegisteredMonitor>| async move {
                    let _ = reg.activate_tx.send(()).await;
                    Ok(())
                })
            });

            ib.method_with_cr_async(
                "DeviceFound",
                ("device",),
                (),
                |ctx, cr, (addr,): (dbus::Path<'static>,)| {
                    method_call(ctx, cr, |reg: Arc<RegisteredMonitor>| async move {
                        let (adapter, device) = Self::parse_device_path(&addr)?;
                        if let Some(event_tx) = reg.event_tx.lock().await.as_ref() {
                            let _ = event_tx.send(MonitorEvent::DeviceFound(DeviceId { adapter, device })).await;
                        }
                        Ok(())
                    })
                },
            );

            ib.method_with_cr_async("DeviceLost", ("device",), (), |ctx, cr, (addr,): (dbus::Path<'static>,)| {
                method_call(ctx, cr, move |reg: Arc<RegisteredMonitor>| async move {
                    let (adapter, device) = Self::parse_device_path(&addr)?;
                    if let Some(event_tx) = reg.event_tx.lock().await.as_ref() {
                        let _ = event_tx.send(MonitorEvent::DeviceLost(DeviceId { adapter, device })).await;
                    }
                    Ok(())
                })
            });

            cr_property!(ib, "Type", r => {
                Some(r.am.monitor_type.to_string())
            });

            cr_property!(ib, "RSSILowThreshold", r => {
                r.am.rssi_low_threshold
            });

            cr_property!(ib, "RSSIHighThreshold", r => {
                r.am.rssi_high_threshold
            });

            cr_property!(ib, "RSSILowTimeout", r => {
                r.am.rssi_low_timeout.map(|t| t.as_secs().clamp(1, 300) as u16)
            });

            cr_property!(ib, "RSSIHighTimeout", r => {
                r.am.rssi_high_timeout.map(|t| t.as_secs().clamp(1, 300) as u16)
            });

            cr_property!(ib, "RSSISamplingPeriod", r => {
                r.am.rssi_sampling_period.map(|v| v.to_value())
            });

            cr_property!(ib, "Patterns", r => {
                r.am.patterns.as_ref().map(|patterns: &Vec<Pattern>| {
                    patterns
                        .iter()
                        .map(|p| (p.start_position, p.data_type, p.content.clone()))
                        .collect::<Vec<_>>()
                })
            });
        })
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
    name: dbus::Path<'static>,
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
    root: dbus::Path<'static>,
    _drop_tx: oneshot::Sender<()>,
}

impl fmt::Debug for MonitorManager {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("MonitorManager").finish()
    }
}

impl MonitorManager {
    pub(crate) async fn new(inner: Arc<SessionInner>, adapter_name: &str) -> Result<Self> {
        let manager_path = dbus::Path::new(format!("{}/{}", MANAGER_PATH, adapter_name)).unwrap();
        let root = dbus::Path::new(format!("{}/{}", MONITOR_PREFIX, Uuid::new_v4().as_simple())).unwrap();

        log::trace!("Publishing advertisement monitor root at {}", &root);

        {
            let mut cr = inner.crossroads.lock().await;
            let object_manager_token = cr.object_manager();
            let introspectable_token = cr.introspectable();
            let properties_token = cr.properties();
            cr.insert(root.clone(), [&object_manager_token, &introspectable_token, &properties_token], ());
        }

        log::trace!("Registering advertisement monitor root at {}", &root);
        let proxy = Proxy::new(SERVICE_NAME, manager_path, TIMEOUT, inner.connection.clone());
        proxy.method_call(MANAGER_INTERFACE, "RegisterMonitor", (root.clone(),)).await?;

        let (_drop_tx, drop_rx) = oneshot::channel();
        let unreg_root = root.clone();
        let unreg_inner = inner.clone();
        tokio::spawn(async move {
            let _ = drop_rx.await;

            log::trace!("Unregistering advertisement monitor root at {}", &unreg_root);
            let _: std::result::Result<(), dbus::Error> =
                proxy.method_call(MANAGER_INTERFACE, "UnregisterMonitor", (unreg_root.clone(),)).await;

            log::trace!("Unpublishing advertisement monitor root at {}", &unreg_root);
            let mut cr = unreg_inner.crossroads.lock().await;
            cr.remove::<()>(&unreg_root);
        });

        Ok(Self { inner, root, _drop_tx })
    }

    /// Registers an advertisement monitor target.
    ///
    /// Returns a handle to receive events.
    pub async fn register(&self, advertisement_monitor: Monitor) -> Result<MonitorHandle> {
        let name = dbus::Path::new(format!("{}/{}", &self.root, Uuid::new_v4().as_simple())).unwrap();

        log::trace!("Publishing advertisement monitor target at {}", &name);

        let (activate_tx, mut activate_rx) = mpsc::channel(1);
        let (release_tx, mut release_rx) = mpsc::channel(1);
        let (event_tx, event_rx) = mpsc::channel(1024);
        let (_drop_tx, drop_rx) = oneshot::channel();

        let reg = RegisteredMonitor {
            am: advertisement_monitor,
            activate_tx,
            release_tx,
            event_tx: Mutex::new(Some(event_tx)),
        };

        {
            let mut cr = self.inner.crossroads.lock().await;
            cr.insert(name.clone(), [&self.inner.monitor_token], Arc::new(reg));
        }

        let inner = self.inner.clone();
        let unreg_name = name.clone();
        tokio::spawn(async move {
            let _ = drop_rx.await;

            log::trace!("Unpublishing advertisement monitor target at {}", &unreg_name);
            let mut cr = inner.crossroads.lock().await;
            cr.remove::<Arc<RegisteredMonitor>>(&unreg_name);
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
