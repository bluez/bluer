//! Bluetooth mesh provisioner.

use dbus::nonblock::{Proxy, SyncConnection};
use dbus_crossroads::{Crossroads, IfaceBuilder, IfaceToken};
use std::{str::FromStr, sync::Arc};
use tokio::sync::Mutex;
use uuid::Uuid;

use super::application::RegisteredApplication;
use crate::{
    mesh::{
        management::{AddNodeFailedReason, NodeAdded},
        ReqError, PATH, SERVICE_NAME, TIMEOUT,
    },
    method_call, SessionInner,
};

pub(crate) const INTERFACE: &str = "org.bluez.mesh.Provisioner1";

/// Bluetooth mesh provisioner.
#[derive(Debug, Clone, Default)]
pub struct Provisioner {
    /// Subnet index of the net_key.
    pub net_index: u16,
    /// Start address for this provisioner.
    pub start_address: u16,
    #[doc(hidden)]
    pub _non_exclusive: (),
}

/// A provisioner exposed over D-Bus to bluez.
pub(crate) struct RegisteredProvisioner {
    inner: Arc<SessionInner>,
    provisioner: Provisioner,
    next_address: Mutex<u16>,
}

impl RegisteredProvisioner {
    pub(crate) fn new(inner: Arc<SessionInner>, provisioner: Provisioner) -> Self {
        Self { inner, provisioner: provisioner.clone(), next_address: Mutex::new(provisioner.start_address) }
    }

    fn proxy(&self) -> Proxy<'_, &SyncConnection> {
        Proxy::new(SERVICE_NAME, PATH, TIMEOUT, &*self.inner.connection)
    }

    dbus_interface!();
    dbus_default_interface!(INTERFACE);

    pub(crate) fn register_interface(cr: &mut Crossroads) -> IfaceToken<Arc<RegisteredApplication>> {
        cr.register(INTERFACE, |ib: &mut IfaceBuilder<Arc<RegisteredApplication>>| {
            ib.method_with_cr_async(
                "AddNodeComplete",
                ("uuid", "unicast", "count"),
                (),
                |ctx, cr, (uuid, unicast, count): (Vec<u8>, u16, u8)| {
                    method_call(ctx, cr, move |reg: Arc<RegisteredApplication>| async move {
                        let uuid = Uuid::from_slice(&uuid).map_err(|_| ReqError::Failed)?;
                        reg.add_node_result_tx
                            .send((uuid, Ok(NodeAdded { unicast, count: count.into() })))
                            .map_err(|_| ReqError::Failed)?;
                        Ok(())
                    })
                },
            );

            ib.method_with_cr_async(
                "AddNodeFailed",
                ("uuid", "reason"),
                (),
                |ctx, cr, (uuid, reason): (Vec<u8>, String)| {
                    method_call(ctx, cr, move |reg: Arc<RegisteredApplication>| async move {
                        let uuid = Uuid::from_slice(&uuid).map_err(|_| ReqError::Failed)?;
                        let reason =
                            AddNodeFailedReason::from_str(&reason).unwrap_or(AddNodeFailedReason::Unknown);
                        reg.add_node_result_tx.send((uuid, Err(reason))).map_err(|_| ReqError::Failed)?;
                        Ok(())
                    })
                },
            );

            ib.method_with_cr_async(
                "RequestProvData",
                ("count",),
                ("net_index", "unicast"),
                |ctx, cr, (count,): (u8,)| {
                    method_call(ctx, cr, move |reg: Arc<RegisteredApplication>| async move {
                        match &reg.provisioner {
                            Some(prov) => {
                                let mut next_addr = prov.next_address.lock().await;
                                let addr = *next_addr;
                                *next_addr += u16::from(count) + 1;
                                Ok((prov.provisioner.net_index, addr))
                            }
                            None => Err(dbus::MethodErr::from(ReqError::Failed)),
                        }
                    })
                },
            );

            cr_property!(ib, "VersionID", _reg => {
                Some(1u16)
            });
        })
    }
}
