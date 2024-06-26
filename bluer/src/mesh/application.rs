//! Bluetooth mesh application.

use dbus::{
    nonblock::{Proxy, SyncConnection},
    Path,
};
use dbus_crossroads::{Crossroads, IfaceBuilder, IfaceToken};
use std::{fmt, sync::Arc};
use strum::EnumString;
use tokio::sync::{broadcast, mpsc, oneshot};
use uuid::Uuid;

use super::{
    agent::{ProvisionAgent, RegisteredProvisionAgent},
    management::{AddNodeFailedReason, NodeAdded},
    provisioner::{Provisioner, RegisteredProvisioner},
};
use crate::{
    mesh::{
        element::{Element, RegisteredElement},
        PATH, SERVICE_NAME, TIMEOUT,
    },
    method_call, Error, ErrorKind, Result, SessionInner,
};

pub(crate) const INTERFACE: &str = "org.bluez.mesh.Application1";
pub(crate) const MESH_APP_PREFIX: &str = publish_path!("mesh/app/");

/// Definition of Bluetooth mesh application.
#[derive(Debug, Default)]
pub struct Application {
    /// Device ID
    pub device_id: Uuid,
    /// Application elements
    pub elements: Vec<Element>,
    /// Provisioner
    pub provisioner: Option<Provisioner>,
    /// Provisioning agent.
    pub agent: ProvisionAgent,
    /// Application properties
    pub properties: Properties,
    #[doc(hidden)]
    pub _non_exhaustive: (),
}

/// Application properties.
#[derive(Debug, Clone, Default)]
pub struct Properties {
    /// Company id.
    pub company_id: u16,
    /// Product id.
    pub product_id: u16,
    /// Version id.
    pub version_id: u16,
}

// ---------------
// D-Bus interface
// ---------------

/// Reason why node provisioning initiated by joining has failed.
#[derive(Debug, displaydoc::Display, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, EnumString)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum JoinFailedReason {
    /// timeout
    #[strum(serialize = "timeout")]
    Timeout,
    /// bad PDU
    #[strum(serialize = "bad-pdu")]
    BadPdu,
    /// confirmation failure
    #[strum(serialize = "confirmation-failed")]
    ConfirmationFailed,
    /// out of resources
    #[strum(serialize = "out-of-resources")]
    OutOfResources,
    /// decryption error
    #[strum(serialize = "decryption-error")]
    DecryptionError,
    /// unexpected error
    #[strum(serialize = "unexpected-error")]
    UnexpectedError,
    /// cannot assign addresses
    #[strum(serialize = "cannot-assign-addresses")]
    CannotAssignAddresses,
    /// Unknown reason
    Unknown,
}

impl From<JoinFailedReason> for Error {
    fn from(reason: JoinFailedReason) -> Self {
        Error::new(ErrorKind::MeshJoinFailed(reason))
    }
}

pub(crate) struct RegisteredApplication {
    inner: Arc<SessionInner>,
    device_id: Uuid,
    pub(crate) provisioner: Option<RegisteredProvisioner>,
    properties: Properties,
    join_result_tx: mpsc::Sender<std::result::Result<u64, JoinFailedReason>>,
    pub(crate) add_node_result_tx: broadcast::Sender<(Uuid, std::result::Result<NodeAdded, AddNodeFailedReason>)>,
}

impl RegisteredApplication {
    fn root_path(&self) -> String {
        format!("{}{}", MESH_APP_PREFIX, self.device_id.as_simple())
    }

    pub(crate) fn dbus_path(&self) -> Path<'static> {
        Path::new(self.root_path()).unwrap()
    }

    pub(crate) fn app_dbus_path(&self) -> Path<'static> {
        let app_path = format!("{}/application", self.root_path());
        Path::new(app_path).unwrap()
    }

    pub(crate) fn element_dbus_path(&self, element_idx: usize) -> Path<'static> {
        let element_path = format!("{}/ele{}", self.root_path(), element_idx);
        Path::new(element_path).unwrap()
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
                    let _ = reg.join_result_tx.send(Ok(token)).await;
                    Ok(())
                })
            });

            ib.method_with_cr_async("JoinFailed", ("reason",), (), |ctx, cr, (reason,): (String,)| {
                method_call(ctx, cr, move |reg: Arc<Self>| async move {
                    let _ = reg
                        .join_result_tx
                        .send(Err(reason.parse::<JoinFailedReason>().unwrap_or(JoinFailedReason::Unknown)))
                        .await;
                    Ok(())
                })
            });

            cr_property!(ib, "CompanyID", reg => {
                Some(reg.properties.company_id)
            });

            cr_property!(ib, "ProductID", reg => {
                Some(reg.properties.product_id)
            });

            cr_property!(ib, "VersionID", reg => {
                Some(reg.properties.version_id)
            });
        })
    }

    pub(crate) async fn register(inner: Arc<SessionInner>, app: Application) -> Result<ApplicationHandle> {
        let Application { device_id, elements, provisioner, agent, properties, .. } = app;

        let (join_result_tx, join_result_rx) = mpsc::channel(1);
        let (add_node_result_tx, add_node_result_rx) = broadcast::channel(1024);
        let this = Arc::new(Self {
            inner: inner.clone(),
            device_id,
            provisioner: provisioner.map(|prov| RegisteredProvisioner::new(inner.clone(), prov)),
            properties,
            join_result_tx,
            add_node_result_tx,
        });
        let app_inner = Arc::new(ApplicationInner { add_node_result_rx });

        let root_path = this.dbus_path();
        log::trace!("Publishing mesh application at {}", &root_path);

        {
            let mut cr = inner.crossroads.lock().await;

            // register object manager
            let om = cr.object_manager();
            cr.insert(root_path.clone(), &[om], ());

            // register agent
            cr.insert(
                Path::from(format!("{}/{}", root_path.clone(), "agent")),
                &[inner.provision_agent_token],
                Arc::new(RegisteredProvisionAgent::new(agent, inner.clone())),
            );

            // register application
            let mut ifaces = vec![inner.application_token];
            if this.provisioner.is_some() {
                ifaces.push(inner.provisioner_token);
            }
            cr.insert(this.app_dbus_path(), &[inner.application_token], this.clone());

            // register elements
            for (element_idx, element) in elements.into_iter().enumerate() {
                let element_path = this.element_dbus_path(element_idx);
                let reg_element = RegisteredElement::new(inner.clone(), this.root_path(), element, element_idx);
                cr.insert(element_path.clone(), &[inner.element_token], Arc::new(reg_element));
            }
        }

        let (drop_tx, drop_rx) = oneshot::channel();
        let path_unreg = root_path.clone();
        tokio::spawn(async move {
            let _ = drop_rx.await;

            log::trace!("Unpublishing mesh application at {}", &path_unreg);
            let mut cr = inner.crossroads.lock().await;
            cr.remove::<Self>(&path_unreg);
        });

        Ok(ApplicationHandle {
            app_inner,
            name: root_path,
            device_id,
            token: None,
            join_result_rx,
            _drop_tx: drop_tx,
        })
    }
}

pub(crate) struct ApplicationInner {
    pub add_node_result_rx: broadcast::Receiver<(Uuid, std::result::Result<NodeAdded, AddNodeFailedReason>)>,
}

/// Handle to Bluetooth mesh application.
///
/// Drop this handle to unpublish.
#[must_use = "ApplicationHandle must be held for mesh application to be published"]
pub struct ApplicationHandle {
    pub(crate) app_inner: Arc<ApplicationInner>,
    pub(crate) name: dbus::Path<'static>,
    pub(crate) device_id: Uuid,
    pub(crate) token: Option<u64>,
    pub(crate) join_result_rx: mpsc::Receiver<std::result::Result<u64, JoinFailedReason>>,
    _drop_tx: oneshot::Sender<()>,
}

impl fmt::Debug for ApplicationHandle {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("ApplicationHandle")
            .field("name", &self.name)
            .field("device_id", &self.device_id)
            .field("token", &self.token)
            .finish()
    }
}

impl ApplicationHandle {
    /// Token.
    ///
    /// Only available when application was registered using [`Network::join`](super::network::Network::join).
    ///
    /// The token parameter serves as a unique identifier of the
    /// particular node. The token must be preserved by the application
    /// in order to authenticate itself to the mesh daemon and attach to
    /// the network as a mesh node by calling Attach() method or
    /// permanently remove the identity of the mesh node by calling
    /// Leave() method.
    pub fn token(&self) -> Option<u64> {
        self.token
    }
}

impl Drop for ApplicationHandle {
    fn drop(&mut self) {
        // required for drop order
    }
}
