//! Bluetooth monitor agent.


use dbus::{
    nonblock::{Proxy, SyncConnection},
    Path,
};

use dbus_crossroads::{Crossroads, IfaceBuilder, IfaceToken};
use futures::{pin_mut, Future};
use std::{fmt, pin::Pin, sync::Arc};
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
    Box<dyn (Fn(DeviceFound) -> Pin<Box<dyn Future<Output = ReqResult<String>> + Send>>) + Send + Sync>;

#[derive(Debug)]
#[non_exhaustive]
pub struct DeviceLost {
    /// Adapter making the request.
    pub adapter: String,
    /// Address of device making the request.
    pub addr: Address,
}

pub type DeviceLostFn =
    Box<dyn (Fn(DeviceLost) -> Pin<Box<dyn Future<Output = ReqResult<String>> + Send>>) + Send + Sync>;

pub struct MonitorCallbacks {
    pub release: Option<ReleaseFn>,
    pub activate: Option<ActivateFn>,
    pub device_found: Option<DeviceFoundFn>,
    pub device_lost: Option<DeviceLostFn>,
}

impl Default for MonitorCallbacks {
    fn default() -> MonitorCallbacks {
        MonitorCallbacks {
            release: Option::None,
            activate: Option::None,
            device_found: Option::None,
            device_lost: Option::None,
        }
    }
}

pub struct Monitor {
    inner: Arc<SessionInner>,
    dbus_path: Path<'static>,
    callbacks: MonitorCallbacks
}


impl Monitor {

    pub(crate) fn new(inner: Arc<SessionInner>, callbacks: MonitorCallbacks) -> Self {
        let name = dbus::Path::new(format!("{}/{}", MANAGER_PATH,Uuid::new_v4().as_simple())).unwrap();
        Self { inner: inner, callbacks: callbacks, dbus_path: name }
    }

    fn proxy(&self) -> Proxy<'_, &SyncConnection> {
        Proxy::new(SERVICE_NAME, self.dbus_path, TIMEOUT, &*self.inner.connection)
    }

    dbus_interface!();
    dbus_default_interface!(INTERFACE);
}

pub(crate) struct RegisteredMonitor {
    m: Monitor,
    cancel: Mutex<Option<oneshot::Sender<()>>>,
}

impl RegisteredMonitor {
    pub(crate) fn new(monitor: Monitor) -> Self {
        Self { m: monitor, cancel: Mutex::new(None) }
    }

    async fn get_cancel(&self) -> oneshot::Receiver<()> {
        let (cancel_tx, cancel_rx) = oneshot::channel();
        *self.cancel.lock().await = Some(cancel_tx);
        cancel_rx
    }

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

    pub(crate) fn register_interface(cr: &mut Crossroads) -> IfaceToken<Arc<Self>> {
        cr.register(INTERFACE, |ib: &mut IfaceBuilder<Arc<Self>>| {
            ib.method_with_cr_async(
                "Release",
                (),
                (),
                |ctx, cr, ()| {
                    method_call(ctx, cr, |reg: Arc<Self>| async move {
                        reg.call_no_params(&reg.m.callbacks.release,).await?;
                        Ok(())
                    })
                },
            );
            ib.method_with_cr_async(
                "Activate",
                (),
                (),
                |ctx, cr, ()| {
                    method_call(ctx, cr, |reg: Arc<Self>| async move {
                        reg.call_no_params(
                            &reg.m.callbacks.activate, )
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
                    method_call(ctx, cr, |reg: Arc<Self>| async move {
                        let (adapter, addr) = Self::parse_device_path(&addr)?;
                        reg.call(&reg.m.callbacks.device_found, DeviceFound { adapter, addr },)
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
                    method_call(ctx, cr, move |reg: Arc<Self>| async move {
                        let (adapter, addr) = Self::parse_device_path(&addr)?;
                        reg.call(
                            &reg.m.callbacks.device_lost,
                            DeviceLost { adapter, addr },
                        )
                        .await?;
                        Ok(())
                    })
                },
            );
        })
    }

    pub(crate) async fn register(self, inner: Arc<SessionInner>, adapter_name: &str) -> Result<MonitorHandle> {
        let manager_path = dbus::Path::new(format!("{}/{}", MANAGER_PATH, adapter_name)).unwrap();
        let name = self.m.dbus_path;

        log::trace!("Publishing monitor at {}", &name);

        {
            let mut cr = inner.crossroads.lock().await;
            cr.insert(name.clone(), &[inner.monitor_token], Arc::new(self));
        }

        log::trace!("Registering monitor at {}", &name);
        let proxy = Proxy::new(SERVICE_NAME, manager_path, TIMEOUT, inner.connection.clone());
        proxy.method_call(MANAGER_INTERFACE, "RegisterMonitor", (name.clone(),)).await?;

        let (drop_tx, drop_rx) = oneshot::channel();
        let unreg_name = name.clone();
        tokio::spawn(async move {
            let _ = drop_rx.await;

            log::trace!("Unregistering monitor at {}", &unreg_name);
            let _: std::result::Result<(), dbus::Error> =
                proxy.method_call(MANAGER_INTERFACE, "UnregisterMonitor", (unreg_name.clone(),)).await;

            log::trace!("Unpublishing monitor at {}", &unreg_name);
            let mut cr = inner.crossroads.lock().await;
            let _: Option<Self> = cr.remove(&unreg_name);
        });

        Ok(MonitorHandle { name, _drop_tx: drop_tx })
    }
}

define_properties!(
    Monitor,
    /// Bluetooth monitor properties.
    pub ManitorProperties => {

        // ===========================================================================================
        // Monitor properties
        // ===========================================================================================

        property(
            MonitorType, String,
            dbus: (INTERFACE, "Type", String, MANDATORY),
            get: (monitor_type, v => { v.to_owned() }),
        );

        property(
            RSSILowThreshold, i16,
            dbus: (INTERFACE, "RSSILowThreshold", i16, OPTIONAL),
            get: (rssi_low_threshold, v => {v.to_owned()}),
        );

        property(
            RSSIHighThreshold, i16,
            dbus: (INTERFACE, "RSSIHighThreshold", i16, OPTIONAL),
            get: (rssi_high_threshold, v => {v.to_owned()}),
        );

        property(
            RSSILowTimeout, i16,
            dbus: (INTERFACE, "RSSILowTimeout", i16, OPTIONAL),
            get: (rssi_low_timeout, v => {v.to_owned()}),
        );

        property(
            RSSIHighTimeout, i16,
            dbus: (INTERFACE, "RSSIHighTimeout", i16, OPTIONAL),
            get: (rssi_high_timeout, v => {v.to_owned()}),
        );

        property(
            RSSISamplingPeriod, u16,
            dbus: (INTERFACE, "RSSISamplingPeriod", u16, OPTIONAL),
            get: (rssi_sampling_period, v => {v.to_owned()}),
        );

        property(
            Patterns, Vec<u8>,
            dbus: (INTERFACE, "Patterns", Vec<u8>, OPTIONAL),
            get: (patterns, v => { v.to_owned() }),
        );
   }
);

/// Handle to registered monitor.
///
/// Drop to unregister monitor.
pub struct MonitorHandle {
    name: dbus::Path<'static>,
    _drop_tx: oneshot::Sender<()>,
}

impl Drop for MonitorHandle {
    fn drop(&mut self) {
        // required for drop order
    }
}

impl fmt::Debug for MonitorHandle {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "MonitorHandle {{ {} }}", &self.name)
    }
}