//! Implement Application bluetooth mesh interface

use crate::{mesh::ReqError, method_call, Result, SessionInner};
use std::sync::Arc;

use dbus::{
    nonblock::{Proxy, SyncConnection},
    Path,
};
use dbus_crossroads::{Crossroads, IfaceBuilder, IfaceToken};
use tokio::sync::mpsc::Sender;

use crate::mesh::{
    element::{Element, RegisteredElement},
    PATH, SERVICE_NAME, TIMEOUT,
};
use futures::channel::oneshot;
use std::{fmt, mem::take};

use super::{
    agent::{ProvisionAgent, RegisteredProvisionAgent},
    provisioner::{Provisioner, RegisteredProvisioner},
};
use uuid::Uuid;

pub(crate) const INTERFACE: &str = "org.bluez.mesh.Application1";
pub(crate) const MESH_APP_PREFIX: &str = publish_path!("mesh/app/");

/// Definition of mesh application.
#[derive(Clone)]
pub struct Application {
    /// Device ID
    pub device_id: Uuid,
    /// Application elements
    pub elements: Vec<Element>,
    /// Provisioner
    pub provisioner: Option<Provisioner>,
    /// Application events sender
    pub events_tx: Sender<ApplicationMessage>,
    /// Provisioning capabilities
    pub agent: ProvisionAgent,
    /// Application properties
    pub properties: Properties,
}

impl Application {
    fn root_path(&self) -> String {
        format!("{}{}", MESH_APP_PREFIX, self.device_id.as_simple())
    }

    pub(crate) fn dbus_path(&self) -> Result<Path<'static>> {
        Ok(Path::new(self.root_path()).unwrap())
    }

    pub(crate) fn app_dbus_path(&self) -> Result<Path<'static>> {
        let app_path = format!("{}/application", self.root_path());
        Ok(Path::new(app_path).unwrap())
    }

    pub(crate) fn element_dbus_path(&self, element_idx: usize) -> Result<Path<'static>> {
        let element_path = format!("{}/ele{}", self.root_path(), element_idx);
        Ok(Path::new(element_path).unwrap())
    }
}

/// Application properties
#[derive(Clone)]
pub struct Properties {
    /// CompanyId
    pub company: u16,
    /// ProductId
    pub product: u16,
    /// VersionId
    pub version: u16,
}

impl Default for Properties {
    fn default() -> Self {
        Self {
            company: 0x05f1 as u16, // The Linux Foundation
            product: 0x0001 as u16,
            version: 0x0001 as u16,
        }
    }
}

// ---------------
// D-Bus interface
// ---------------

/// An Application exposed over D-Bus to bluez.
#[derive(Clone)]
pub struct RegisteredApplication {
    inner: Arc<SessionInner>,
    app: Application,
    agent: RegisteredProvisionAgent,
    /// Registered provisioner
    pub provisioner: Option<RegisteredProvisioner>,
}

impl RegisteredApplication {
    pub(crate) fn new(inner: Arc<SessionInner>, app: Application) -> Self {
        let provisioner = match app.clone().provisioner {
            Some(prov) => Some(RegisteredProvisioner::new(inner.clone(), prov.clone())),
            None => None,
        };
        let agent = RegisteredProvisionAgent::new(app.agent.clone(), inner.clone());

        Self { inner, app, provisioner, agent }
    }

    fn proxy(&self) -> Proxy<'_, &SyncConnection> {
        Proxy::new(SERVICE_NAME, PATH, TIMEOUT, &*self.inner.connection)
    }

    dbus_interface!();
    dbus_default_interface!(INTERFACE);

    pub(crate) fn register_interface(cr: &mut Crossroads) -> IfaceToken<Arc<Self>> {
        cr.register(INTERFACE, |ib: &mut IfaceBuilder<Arc<Self>>| {
            ib.method_with_cr_async("JoinComplete", ("token",), (), |ctx, cr, (token,): (u64,)| {
                method_call(ctx, cr, move |reg: Arc<Self>| async move {
                    reg.app
                        .events_tx
                        .send(ApplicationMessage::JoinComplete(token))
                        .await
                        .map_err(|_| ReqError::Failed)?;
                    Ok(())
                })
            });
            ib.method_with_cr_async("JoinFailed", ("reason",), (), |ctx, cr, (reason,): (String,)| {
                method_call(ctx, cr, move |reg: Arc<Self>| async move {
                    reg.app
                        .events_tx
                        .send(ApplicationMessage::JoinFailed(reason))
                        .await
                        .map_err(|_| ReqError::Failed)?;
                    Ok(())
                })
            });
            cr_property!(ib, "CompanyID", reg => {
                Some(reg.app.properties.company)
            });
            cr_property!(ib, "ProductID", reg => {
                Some(reg.app.properties.product)
            });
            cr_property!(ib, "VersionID", reg => {
                Some(reg.app.properties.version)
            });
        })
    }

    pub(crate) async fn register(mut self, inner: Arc<SessionInner>) -> Result<ApplicationHandle> {
        let root_path = self.app.dbus_path()?;
        log::trace!("Publishing application at {}", &root_path);

        {
            let mut cr = inner.crossroads.lock().await;

            let elements = take(&mut self.app.elements);

            // register object manager
            let om = cr.object_manager();
            cr.insert(root_path.clone(), &[om], ());

            // register agent
            cr.insert(
                Path::from(format!("{}/{}", root_path.clone(), "agent")),
                &[inner.provision_agent_token],
                Arc::new(self.clone().agent),
            );

            // register application
            let app_path = self.app.app_dbus_path()?;
            match self.clone().provisioner {
                Some(_) => cr.insert(
                    app_path.clone(),
                    &[inner.provisioner_token, inner.application_token],
                    Arc::new(self.clone()),
                ),
                None => cr.insert(app_path.clone(), &[inner.application_token], Arc::new(self.clone())),
            }

            for (element_idx, element) in elements.into_iter().enumerate() {
                let element_path = self.app.element_dbus_path(element_idx)?;
                let reg_element = RegisteredElement::new(inner.clone(), element.clone(), element_idx as u8);
                cr.insert(element_path.clone(), &[inner.element_token], Arc::new(reg_element));
            }
        }

        let (drop_tx, drop_rx) = oneshot::channel();
        let path_unreg = root_path.clone();
        tokio::spawn(async move {
            let _ = drop_rx.await;

            log::trace!("Unpublishing application at {}", &path_unreg);
            let mut cr = inner.crossroads.lock().await;
            let _: Option<Self> = cr.remove(&path_unreg);
        });

        Ok(ApplicationHandle { name: root_path, _drop_tx: drop_tx })
    }
}

/// Handle to Application
///
/// Drop this handle to unpublish.
pub struct ApplicationHandle {
    pub(crate) name: dbus::Path<'static>,
    _drop_tx: oneshot::Sender<()>,
}

impl Drop for ApplicationHandle {
    fn drop(&mut self) {
        // required for drop order
    }
}

impl fmt::Debug for ApplicationHandle {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ApplicationHandle {{ {} }}", &self.name)
    }
}

#[derive(Clone, Debug)]
/// Messages corresponding to provisioner method calls during the process of joining the node to the network.
pub enum ApplicationMessage {
    /// This method is called when the node provisioning initiated by a Join() method call successfully completed.
    /// The token parameter serves as a unique identifier of the particular node.
    JoinComplete(u64),
    ///	This method is called when the node provisioning initiated by Join() has failed.
    /// The reason parameter identifies the reason for provisioning failure.
    JoinFailed(String),
}
