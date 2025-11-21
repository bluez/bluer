//! Bluetooth Mesh support.
//!
//! The current implementation is experimental, incomplete and subject to change.
//!

pub mod agent;
pub mod application;
pub mod element;
pub mod management;
pub mod network;
pub mod node;
pub mod provisioner;

// use std::time::Duration;
use strum::IntoStaticStr;

// use crate::ERR_PREFIX;

pub(crate) const SERVICE_NAME: &str = "org.bluez.mesh";
pub(crate) const PATH: &str = "/org/bluez/mesh";
// pub(crate) const TIMEOUT: Duration = Duration::from_secs(120);

// ===========================================================================================
// Request error
// ===========================================================================================

/// Error response from us to a Bluetooth request.
#[derive(
    Clone, Copy, Debug, displaydoc::Display, Eq, PartialEq, Ord, PartialOrd, Hash, IntoStaticStr, Default,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum ReqError {
    /// Bluetooth request failed
    #[default]
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

impl From<ReqError> for zbus::fdo::Error {
    fn from(err: ReqError) -> Self {
        match err {
            ReqError::Failed => zbus::fdo::Error::Failed("Failed".into()),
            ReqError::InProgress => zbus::fdo::Error::Failed("In Progress".into()),
            ReqError::InvalidOffset => zbus::fdo::Error::InvalidArgs("Invalid Offset".into()),
            ReqError::InvalidValueLength => zbus::fdo::Error::InvalidArgs("Invalid Value Length".into()),
            ReqError::NotPermitted => zbus::fdo::Error::AccessDenied("Not Permitted".into()),
            ReqError::NotAuthorized => zbus::fdo::Error::AccessDenied("Not Authorized".into()),
            ReqError::NotSupported => zbus::fdo::Error::NotSupported("Not Supported".into()),
        }
    }
}

/// Result of a Bluetooth request to us.
pub type ReqResult<T> = std::result::Result<T, ReqError>;
