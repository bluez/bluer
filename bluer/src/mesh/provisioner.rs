//! Implement Provisioner bluetooth mesh interface

use crate::{mesh::ReqError, method_call, SessionInner};
use std::sync::{Arc, Mutex};

use dbus::nonblock::{Proxy, SyncConnection};
use dbus_crossroads::{Crossroads, IfaceBuilder, IfaceToken};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::mesh::{PATH, SERVICE_NAME, TIMEOUT};

use super::application::RegisteredApplication;

pub(crate) const INTERFACE: &str = "org.bluez.mesh.Provisioner1";

/// Definition of Provisioner interface
#[derive(Clone)]
pub struct Provisioner {
    /// Start address for this provisioner
    pub start_address: i32,
    /// Control handle for provisioner once it has been registered.
    pub control_handle: ProvisionerControlHandle,
}

/// A provisioner exposed over D-Bus to bluez.
#[derive(Clone)]
pub struct RegisteredProvisioner {
    inner: Arc<SessionInner>,
    provisioner: Provisioner,
    next_address: Arc<Mutex<i32>>,
}

impl RegisteredProvisioner {
    pub(crate) fn new(inner: Arc<SessionInner>, provisioner: Provisioner) -> Self {
        Self {
            inner,
            provisioner: provisioner.clone(),
            next_address: Arc::new(Mutex::new(provisioner.start_address.clone())),
        }
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
                        if let Some(prov) = &reg.provisioner {
                            prov.provisioner
                                .control_handle
                                .messages_tx
                                .send(ProvisionerMessage::AddNodeComplete(
                                    Uuid::from_slice(&uuid).map_err(|_| ReqError::Failed)?,
                                    unicast,
                                    count,
                                ))
                                .await
                                .map_err(|_| ReqError::Failed)?;
                        }
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
                        if let Some(prov) = &reg.provisioner {
                            prov.provisioner
                                .control_handle
                                .messages_tx
                                .send(ProvisionerMessage::AddNodeFailed(
                                    Uuid::from_slice(&uuid).map_err(|_| ReqError::Failed)?,
                                    reason,
                                ))
                                .await
                                .map_err(|_| ReqError::Failed)?;
                        }
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
                                let adr = prov.next_address.clone();
                                let mut adr = adr.lock().unwrap();
                                let res = (0x000 as u16, *adr as u16);
                                *adr += (count + 1) as i32;
                                Ok(res)
                            }
                            None => Err(dbus::MethodErr::from(ReqError::Failed)),
                        }
                    })
                },
            );
            cr_property!(ib, "VersionID", _reg => {
                Some(0x0001 as u16)
            });
        })
    }
}

#[derive(Clone)]
/// A handle to store inside a provisioner definition to make it controllable
/// once it has been registered.
pub struct ProvisionerControlHandle {
    /// Provisioner messages sender
    pub messages_tx: mpsc::Sender<ProvisionerMessage>,
}

#[derive(Clone, Debug)]
///Messages sent by provisioner
pub enum ProvisionerMessage {
    /// Add node succeded
    AddNodeComplete(Uuid, u16, u8),
    /// Add node failed
    AddNodeFailed(Uuid, String),
}
