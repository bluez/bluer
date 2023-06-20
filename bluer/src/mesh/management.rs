//! Bluetooth mesh management.

use dbus::{
    arg::{RefArg, Variant},
    nonblock::{Proxy, SyncConnection},
    Path,
};
use std::{collections::HashMap, sync::Arc};
use strum::EnumString;
use uuid::Uuid;

use super::application::ApplicationInner;
use crate::{
    mesh::{SERVICE_NAME, TIMEOUT},
    Error, ErrorKind, Result, SessionInner,
};

pub(crate) const INTERFACE: &str = "org.bluez.mesh.Management1";

/// Interface to Bluetooth mesh management.
#[derive(Clone)]
pub struct Management {
    inner: Arc<SessionInner>,
    app_inner: Arc<ApplicationInner>,
    path: Path<'static>,
}

impl Management {
    pub(crate) fn new(inner: Arc<SessionInner>, app_inner: Arc<ApplicationInner>, path: Path<'static>) -> Self {
        Self { inner, app_inner, path }
    }

    /// Add the unprovisioned device specified by UUID to the network.
    pub async fn add_node(&self, uuid: Uuid) -> Result<NodeAdded> {
        let mut rx = self.app_inner.add_node_result_rx.resubscribe();

        let opts = HashMap::<String, Variant<Box<dyn RefArg + 'static>>>::new();
        self.call_method("AddNode", (uuid.as_bytes().to_vec(), opts)).await?;

        loop {
            match rx.recv().await {
                Ok((res_uuid, Ok(node))) if res_uuid == uuid => break Ok(node),
                Ok((res_uuid, Err(reason))) if res_uuid == uuid => {
                    break Err(Error::new(ErrorKind::MeshAddNodeFailed(reason)))
                }
                Ok(_) => (),
                Err(_) => break Err(Error::new(ErrorKind::Failed)),
            }
        }
    }

    fn proxy(&self) -> Proxy<'_, &SyncConnection> {
        Proxy::new(SERVICE_NAME, self.path.clone(), TIMEOUT, &*self.inner.connection)
    }

    dbus_interface!();
    dbus_default_interface!(INTERFACE);
}

/// Information about an added node.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct NodeAdded {
    /// Primary address that has been assigned to the new node, and the address of it's config server
    pub unicast: u16,
    /// Number of unicast addresses assigned to the new node.
    pub count: u16,
}

/// Reason why adding node has failed.
#[derive(Debug, displaydoc::Display, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, EnumString)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum AddNodeFailedReason {
    /// aborted
    #[strum(serialize = "aborted")]
    Aborted,
    /// timeout
    #[strum(serialize = "timeout")]
    Timeout,
    /// bad PDU
    #[strum(serialize = "bad-pdu")]
    BadPdu,
    /// confirmation failed
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
    /// unknown reason
    Unknown,
}
