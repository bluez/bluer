//! Bluetooth monitor agent.

use dbus::nonblock::Proxy;
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
pub(crate) const AGENT_PREFIX: &str = publish_path!("monitor/");

pub type ReleaseFn =
    Box<dyn (Fn() -> Pin<Box<dyn Future<Output = ReqResult<String>> + Send>>) + Send + Sync>;

pub type ActivateFn =
    Box<dyn (Fn() -> Pin<Box<dyn Future<Output = ReqResult<String>> + Send>>) + Send + Sync>;

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

/// Use [Session::register_monitor](crate::session::Session::register_monitor) to register the handler.
#[derive(Default)]
pub struct Monitor {
    /// Monitor Type.
    pub monitor_type: String,
    pub rssi_low_threshold: i16,
    pub rssi_high_threshold: i16,
    pub rssi_low_timeout: i16,
    pub rssi_high_timeout: i16,
    pub rssi_sampling_period: i16,
    pub patters: Mutext<vec<u8>>,

    pub release: Option<ReleaseFn>,
    pub activate: Option<ActivateFn>,
    pub device_found: Option<DeviceFoundFn>,
    pub device_lost: Option<DeviceLostFn>,
   #[doc(hidden)]
    pub _non_exhaustive: (),
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
                |ctx, cr, (): (dbus::Path<'static>,)| {
                    method_call(ctx, cr, |reg: Arc<Self>| async move {
                        Ok((reg
                            .call(&reg.m.release, () ))
                            .await?,))
                    })
                },
            );
            ib.method_with_cr_async(
                "Activate",
                (),
                (),
                |ctx, cr, (): (dbus::Path<'static>, String)| {
                    method_call(ctx, cr, |reg: Arc<Self>| async move {
                        reg.call(
                            &reg.m.activate,
                            () },
                        )
                        .await?;
                        Ok(())
                    })
                },
            );
            ib.method_with_cr_async(
                "DeviceFound",
                ("device",),
                ("value",),
                |ctx, cr, (device,): (dbus::Path<'static>,)| {
                    method_call(ctx, cr, |reg: Arc<Self>| async move {
                        let (adapter, device) = Self::parse_device_path(&device)?;
                        Ok((reg
                            .call(&reg.m.device_found, DeviceFound { adapter, device })
                            .await?,))
                    })
                },
            );
            ib.method_with_cr_async(
                "DeviceLost",
                ("device",),
                (),
                |ctx, cr, (device,): (dbus::Path<'static>, u32, u16)| {
                    method_call(ctx, cr, move |reg: Arc<Self>| async move {
                        let (adapter, device) = Self::parse_device_path(&device)?;
                        reg.call(
                            &reg.m.device_lost,
                            DeviceLost { adapter, device },
                        )
                        .await?;
                        Ok(())
                    })
                },
            );
        })
    }

    pub(crate) async fn register(self, inner: Arc<SessionInner>) -> Result<AgentHandle> {
        let name = dbus::Path::new(format!("{}{}", AGENT_PREFIX, Uuid::new_v4().as_simple())).unwrap();
        log::trace!("Publishing monitor at {}", &name);

        {
            let mut cr = inner.crossroads.lock().await;
            cr.insert(name.clone(), &[inner.monitor_token], Arc::new(self));
        }

        log::trace!("Registering monitor at {}", &name);
        let proxy = Proxy::new(SERVICE_NAME, MANAGER_PATH, TIMEOUT, inner.connection.clone());
        proxy.method_call(MANAGER_INTERFACE, "RegisterMonitor", (name.clone(),)).await?;
        let connection = inner.connection.clone();

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