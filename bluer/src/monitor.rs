//! Bluetooth monitor agent.


use dbus::{
    nonblock::{Proxy, SyncConnection},
    Path,
};
use strum::{Display, EnumString};
use dbus_crossroads::{Crossroads, IfaceBuilder, IfaceToken};
use futures::{pin_mut, Future};
use std::{fmt, pin::Pin, sync::Arc, collections::HashMap};
use strum::IntoStaticStr;
use tokio::{
    select,
    sync::{oneshot, Mutex},
};
use uuid::Uuid;

use crate::{method_call, Address, Device, Result, SessionInner, ERR_PREFIX, SERVICE_NAME, TIMEOUT};

pub(crate) const INTERFACE: &str = "org.bluez.AdvertisementMonitor1";
pub(crate) const MANAGER_INTERFACE: &str = "org.bluez.AdvertisementMonitorManager1";
pub(crate) const MANAGER_PATH: &str = "/org/bluez";
pub(crate) const MONITOR_PREFIX: &str = publish_path!("monitor");

// Error response from us to a Bluetooth agent request.
#[derive(Clone, Copy, Debug, displaydoc::Display, Eq, PartialEq, Ord, PartialOrd, Hash, IntoStaticStr)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum ReqError {
    /// Request was rejected.
    Rejected,
    /// Request was canceled.
    Canceled,
}

impl std::error::Error for ReqError {}

impl Default for ReqError {
    fn default() -> Self {
        Self::Canceled
    }
}

impl From<ReqError> for dbus::MethodErr {
    fn from(err: ReqError) -> Self {
        let name: &'static str = err.into();
        Self::from((ERR_PREFIX.to_string() + name, &err.to_string()))
    }
}

/// Result of a Bluetooth agent request to us.
pub type ReqResult<T> = std::result::Result<T, ReqError>;

/// Determines the type of advertisement monitor.
#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord, Hash, Display, EnumString)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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

/// An advertisement data pattern, used to filter devices in the advertisement monitor.
#[derive(Clone)]
pub struct Pattern {
    /// The index in an AD data field where the search should start. The
    /// beginning of an AD data field is index 0.
    pub start_position: u8,
    /// Advertising data type to match. See
    /// <https://www.bluetooth.com/specifications/assigned-numbers/generic-access-profile/> for the
    /// possible allowed values.
    pub ad_data_type: u8,
    /// The value of the pattern. The maximum length of the bytes is 31.
    pub content_of_pattern: Vec<u8>,
}

pub type ReleaseFn =
    Box<dyn (Fn() -> Pin<Box<dyn Future<Output = ReqResult<()>> + Send>>) + Send + Sync>;

pub type ActivateFn =
    Box<dyn (Fn() -> Pin<Box<dyn Future<Output = ReqResult<()>> + Send>>) + Send + Sync>;

#[derive(Debug)]
#[non_exhaustive]
pub struct DeviceFound {
    /// Adapter making the request.
    pub adapter: String,
    /// Address of device making the request.
    pub addr: Address,
}

pub type DeviceFoundFn =
    Box<dyn (Fn(DeviceFound) -> Pin<Box<dyn Future<Output = ReqResult<()>> + Send>>) + Send + Sync>;

#[derive(Debug)]
#[non_exhaustive]
pub struct DeviceLost {
    /// Adapter making the request.
    pub adapter: String,
    /// Address of device making the request.
    pub addr: Address,
}

pub type DeviceLostFn =
    Box<dyn (Fn(DeviceLost) -> Pin<Box<dyn Future<Output = ReqResult<()>> + Send>>) + Send + Sync>;

pub struct Monitor {
    pub monitor_type: Type,
    pub rssi_low_threshold: Option<i16>,
    pub rssi_high_threshold: Option<i16>,
    pub rssi_low_timeout: Option<u16>,
    pub rssi_high_timeout: Option<u16>,
    pub rssi_sampling_period: Option<u16>,
    pub patterns: Option<Vec<Pattern>>,
    pub release: Option<ReleaseFn>,
    pub activate: Option<ActivateFn>,
    pub device_found: Option<DeviceFoundFn>,
    pub device_lost: Option<DeviceLostFn>,
}

impl Default for Monitor {
    fn default() -> Monitor {
        Monitor {
            monitor_type: Type::OrPatterns,
            rssi_low_threshold: Some(127),
            rssi_high_threshold: Some(127),
            rssi_low_timeout: Some(0),
            rssi_high_timeout: Some(0),
            rssi_sampling_period: Some(0),
            patterns: Option::None,
            release: Option::None,
            activate: Option::None,
            device_found: Option::None,
            device_lost: Option::None,
        }
    }
}

impl Monitor {
    async fn call<A, F, R>(&self, f: &Option<impl Fn(A) -> F>, arg: A) -> ReqResult<R>
    where
        F: Future<Output = ReqResult<R>> + Send + 'static,
    {
        match f {
            Some(f) => f(arg).await,
            None => Err(ReqError::Rejected),
        }
    }

    async fn call_no_params<F, R>(&self, f: &Option<impl Fn() -> F>) -> ReqResult<R>
    where
        F: Future<Output = ReqResult<R>> + Send + 'static,
    {
        match f {
            Some(f) => f().await,
            None => Err(ReqError::Rejected),
        }
    }
    
    fn parse_device_path(device: &dbus::Path<'static>) -> ReqResult<(String, Address)> {
        match Device::parse_dbus_path(device) {
            Some((adapter, addr)) => Ok((adapter.to_string(), addr)),
            None => {
                log::error!("Cannot parse device path {}", &device);
                Err(ReqError::Rejected)
            }
        }
    }

    pub(crate) fn register_interface(cr: &mut Crossroads) -> IfaceToken<Arc<Monitor>> {
        cr.register(INTERFACE, |ib: &mut IfaceBuilder<Arc<Monitor>>| {
            ib.method_with_cr_async(
                "Release",
                (),
                (),
                |ctx, cr, ()| {
                    method_call(ctx, cr, |reg: Arc<Monitor>| async move {
                        reg.call_no_params(&reg.release,).await?;
                        Ok(())
                    })
                },
            );
            ib.method_with_cr_async(
                "Activate",
                (),
                (),
                |ctx, cr, ()| {
                    method_call(ctx, cr, |reg: Arc<Monitor>| async move {
                        reg.call_no_params(
                            &reg.activate, )
                        .await?;
                        Ok(())
                    })
                },
            );
            ib.method_with_cr_async(
                "DeviceFound",
                ("device",),
                (),
                |ctx, cr, (addr,):(dbus::Path<'static>,) | {
                    method_call(ctx, cr, |reg: Arc<Monitor>| async move {
                        let (adapter, addr) = Self::parse_device_path(&addr)?;
                        reg.call(&reg.device_found, DeviceFound { adapter, addr },)
                        .await?;
                        Ok(())
                    })
                },
            );
            ib.method_with_cr_async(
                "DeviceLost",
                ("device",),
                (),
                |ctx, cr, (addr,): (dbus::Path<'static>,) | {
                    method_call(ctx, cr, move |reg: Arc<Monitor>| async move {
                        let (adapter, addr) = Self::parse_device_path(&addr)?;
                        reg.call(
                            &reg.device_lost,
                            DeviceLost { adapter, addr },
                        )
                        .await?;
                        Ok(())
                    })
                },
            );
            cr_property!(ib,"Type",r => {
                Some(r.monitor_type.to_string())
            });

            cr_property!(ib,"RSSILowThreshold",r => {
                r.rssi_low_threshold
            });

            cr_property!(ib,"RSSIHighThreshold",r => {
                r.rssi_high_threshold
            });

            cr_property!(ib,"RSSILowTimeout",r => {
                r.rssi_low_timeout
            });

            cr_property!(ib,"RSSIHighTimeout",r => {
                r.rssi_high_timeout
            });

            cr_property!(ib,"RSSISamplingPeriod",r => {
                r.rssi_sampling_period
            });

            cr_property!(ib,"Patterns",r => {
                r.patterns.as_ref().map(|patterns: &Vec<Pattern>| {
                    patterns
                        .iter()
                        .map(|p| (p.start_position, p.ad_data_type, p.content_of_pattern.clone()))
                        .collect::<Vec<_>>()
                })
            });
        })
    }    
}

pub(crate) struct RegisteredMonitor {
    monitors: Arc<Mutex<HashMap<dbus::Path<'static>, Arc<Monitor>>>>,
    inner: Arc<SessionInner>
}

impl RegisteredMonitor {
    pub(crate) fn new(inner: Arc<SessionInner>) -> Self {
        Self {monitors: Arc::new(Mutex::new(HashMap::new())), inner: inner.clone()}
    }

    pub(crate) async fn register(self, adapter_name: &str) -> Result<RegisteredMonitorHandle> {
        let manager_path = dbus::Path::new(format!("{}/{}", MANAGER_PATH, adapter_name)).unwrap();
        let root = dbus::Path::new(MONITOR_PREFIX).unwrap();

        log::trace!("Publishing monitor at {}", &root);

        {
            let mut cr = self.inner.crossroads.lock().await;
            let object_manager_token = cr.object_manager();
            let introspectable_token = cr.introspectable();
            let properties_token = cr.properties();
            cr.insert(root.clone(), [&object_manager_token, &introspectable_token, &properties_token], {});
        }

        log::trace!("Registering monitor at {}", &root);
        let proxy = Proxy::new(SERVICE_NAME, manager_path, TIMEOUT, self.inner.connection.clone());
        proxy.method_call(MANAGER_INTERFACE, "RegisterMonitor", (root.clone(),)).await?;

        let (drop_tx, drop_rx) = oneshot::channel();
        let unreg_name = root.clone();

        let r = Arc::new(Mutex::new(self));
        let s = r.clone();
        tokio::spawn(async move {
            let _ = drop_rx.await;

            log::trace!("Unregistering monitor at {}", &unreg_name);
            let _: std::result::Result<(), dbus::Error> =
                proxy.method_call(MANAGER_INTERFACE, "UnregisterMonitor", (unreg_name.clone(),)).await;

            log::trace!("Unpublishing monitor at {}", &unreg_name);
            let sl = s.lock().await;
            let mut cr = sl.inner.crossroads.lock().await;
            let _: Option<Self> = cr.remove(&unreg_name);

            let m = sl.monitors.clone();
            let ml = m.lock().await;
            for (path,_) in ml.iter() {
                let _: Option<Self> = cr.remove(&path);
            }
        });

        Ok(RegisteredMonitorHandle { name: root, r: r, _drop_tx: drop_tx })
    }

    pub async fn add_monitor(&mut self, monitor: Arc<Monitor>) -> Result<MonitorHandle> {
        let name = dbus::Path::new(format!("{}/{}",MONITOR_PREFIX,Uuid::new_v4().as_simple())).unwrap();

        log::trace!("Publishing monitor rule at {}", &name);

        let mut m = self.monitors.lock().await;
        m.insert(name.clone(), monitor.clone());

        let mut cr = self.inner.crossroads.lock().await;
        cr.insert(name.clone(), [&self.inner.monitor_token], monitor.clone());

        Ok(MonitorHandle {path: name.clone()})
    }

    pub async fn del_monitor(&mut self, path: dbus::Path<'static>) {
        let mut cr = self.inner.crossroads.lock().await;
        let _: Option<Self> = cr.remove(&path);

        let mut m = self.monitors.lock().await;
        let _ = m.remove(&path);
    }
}

pub struct MonitorHandle {
    path: dbus::Path<'static>,
}

/// Handle to registered monitor.
///
/// Drop to unregister monitor.
pub struct RegisteredMonitorHandle {
    name: dbus::Path<'static>,
    r: Arc<Mutex<RegisteredMonitor>>,
    _drop_tx: oneshot::Sender<()>,
}

impl RegisteredMonitorHandle {
    pub async fn add_monitor(&mut self, monitor: Monitor) -> Result<MonitorHandle> {
        let mut r = self.r.lock().await;
        r.add_monitor(Arc::new(monitor)).await
    }

    pub async fn del_monitor(&mut self, monitorHandle: MonitorHandle) {
        let mut r = self.r.lock().await;
        r.del_monitor(monitorHandle.path).await;
    }
}

impl Drop for RegisteredMonitorHandle {
    fn drop(&mut self) {
        // required for drop order
    }
}

impl fmt::Debug for RegisteredMonitorHandle {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "MonitorHandle {{ {} }}", &self.name)
    }
}