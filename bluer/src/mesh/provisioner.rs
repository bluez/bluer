//! Bluetooth mesh provisioner.

use std::{str::FromStr, sync::Arc};
use tokio::sync::Mutex;
use uuid::Uuid;
use zbus::{fdo, interface};

use crate::{
    mesh::{
        management::{AddNodeFailedReason, NodeAdded},
        ReqError,
    },
    SessionInner,
};

// pub(crate) const INTERFACE: &str = "org.bluez.mesh.Provisioner1";

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
#[derive(Clone)]
pub(crate) struct RegisteredProvisioner {
    #[allow(dead_code)]
    inner: Arc<SessionInner>,
    provisioner: Provisioner,
    next_address: Arc<Mutex<u16>>,
    add_node_result_tx:
        tokio::sync::broadcast::Sender<(Uuid, std::result::Result<NodeAdded, AddNodeFailedReason>)>,
}

impl RegisteredProvisioner {
    pub(crate) fn new(
        inner: Arc<SessionInner>, provisioner: Provisioner,
        add_node_result_tx: tokio::sync::broadcast::Sender<(
            Uuid,
            std::result::Result<NodeAdded, AddNodeFailedReason>,
        )>,
    ) -> Self {
        Self {
            inner,
            provisioner: provisioner.clone(),
            next_address: Arc::new(Mutex::new(provisioner.start_address)),
            add_node_result_tx,
        }
    }
}

#[interface(name = "org.bluez.mesh.Provisioner1")]
impl RegisteredProvisioner {
    async fn add_node_complete(&self, uuid: Vec<u8>, unicast: u16, count: u8) -> Result<(), fdo::Error> {
        let uuid = Uuid::from_slice(&uuid).map_err(|_| ReqError::Failed)?;
        self.add_node_result_tx
            .send((uuid, Ok(NodeAdded { unicast, count: count.into() })))
            .map_err(|_| ReqError::Failed)?;
        Ok(())
    }

    async fn add_node_failed(&self, uuid: Vec<u8>, reason: String) -> Result<(), fdo::Error> {
        let uuid = Uuid::from_slice(&uuid).map_err(|_| ReqError::Failed)?;
        let reason = AddNodeFailedReason::from_str(&reason).unwrap_or(AddNodeFailedReason::Unknown);
        self.add_node_result_tx.send((uuid, Err(reason))).map_err(|_| ReqError::Failed)?;
        Ok(())
    }

    async fn request_prov_data(&self, count: u8) -> Result<(u16, u16), fdo::Error> {
        let mut next_addr = self.next_address.lock().await;
        let addr = *next_addr;
        *next_addr += u16::from(count) + 1;
        Ok((self.provisioner.net_index, addr))
    }
}
