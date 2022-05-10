//! Bluetooth Mesh module

pub mod agent;
pub mod application;
pub mod element;
pub mod management;
pub mod network;
pub mod node;
pub mod provisioner;
mod types;
pub use types::*;

use crate::{Result, ERR_PREFIX};
use dbus::{
    arg::PropMap,
    nonblock::{stdintf::org_freedesktop_dbus::ObjectManager, Proxy, SyncConnection},
    Path,
};
use std::{collections::HashMap, time::Duration};
use strum::IntoStaticStr;

pub(crate) const SERVICE_NAME: &str = "org.bluez.mesh";
pub(crate) const PATH: &str = "/org/bluez/mesh";
pub(crate) const TIMEOUT: Duration = Duration::from_secs(120);

// ===========================================================================================
// Request error
// ===========================================================================================

/// Error response from us to a Bluetooth request.
#[derive(Clone, Copy, Debug, displaydoc::Display, Eq, PartialEq, Ord, PartialOrd, Hash, IntoStaticStr)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum ReqError {
    /// Bluetooth request failed
    Failed,
    /// Bluetooth request already in progress
    InProgress,
    /// Invalid offset for Bluetooth GATT property
    InvalidOffset,
    /// Invalid value length for Bluetooth GATT property
    InvalidValueLength,
    /// Bluetooth request not permitted
    NotPermitted,
    /// Bluetooth request not authorized
    NotAuthorized,
    /// Bluetooth request not supported
    NotSupported,
}

impl std::error::Error for ReqError {}

impl Default for ReqError {
    fn default() -> Self {
        Self::Failed
    }
}

impl From<ReqError> for dbus::MethodErr {
    fn from(err: ReqError) -> Self {
        let name: &'static str = err.into();
        Self::from((ERR_PREFIX.to_string() + name, &err.to_string()))
    }
}

/// Result of a Bluetooth request to us.
pub type ReqResult<T> = std::result::Result<T, ReqError>;

/// Gets all D-Bus objects from the BlueZ service.
async fn all_dbus_objects(
    connection: &SyncConnection,
) -> Result<HashMap<Path<'static>, HashMap<String, PropMap>>> {
    let p = Proxy::new(SERVICE_NAME, "/", TIMEOUT, connection);
    Ok(p.get_managed_objects().await?)
}
