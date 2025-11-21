//! Bluetooth mesh provisoner agent.

use core::fmt;
use futures::Future;
use std::{fmt::Debug, pin::Pin, str::FromStr, sync::Arc};
use strum::{EnumString, IntoStaticStr};

use crate::{
    mesh::{PATH, SERVICE_NAME, TIMEOUT},
    SessionInner, ERR_PREFIX,
};

pub(crate) const INTERFACE: &str = "org.bluez.mesh.ProvisionAgent1";

/// Error response from us to a Bluetooth agent request.
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

impl From<ReqError> for zbus::fdo::Error {
    fn from(err: ReqError) -> Self {
        let name: &'static str = err.into();
        zbus::fdo::Error::Failed(format!("{}.{}", ERR_PREFIX, name))
    }
}

/// Result of a Bluetooth agent request to us.
pub type ReqResult<T> = std::result::Result<T, ReqError>;

/// Agent static capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumString)]
#[non_exhaustive]
pub enum StaticCapability {
    /// 16 octet alpha array.
    #[strum(serialize = "in-alpha")]
    InAlpha,
    /// 16 octet array.
    #[strum(serialize = "static-oob")]
    StaticOob,
}

/// Agent numeric capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumString)]
#[non_exhaustive]
pub enum NumericCapability {
    /// LED blinks.
    #[strum(serialize = "blink")]
    Blink,
    /// Device beeps.
    #[strum(serialize = "beep")]
    Beep,
    /// Device vibrations.
    #[strum(serialize = "vibrate")]
    Vibrate,
    /// Remote value.
    #[strum(serialize = "out-numeric")]
    OutNumeric,
    /// Button pushes.
    #[strum(serialize = "push")]
    Push,
    /// Knob twists.
    #[strum(serialize = "twist")]
    Twist,
}

/// Agent capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Capability {
    /// Static capability.
    Static(StaticCapability),
    /// Numeric capability.
    Numeric(NumericCapability),
}

impl fmt::Display for Capability {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Static(c) => c.fmt(f),
            Self::Numeric(c) => c.fmt(f),
        }
    }
}

/// Function handling a static OOB authentication.
///
/// The Static data returned must be 16 octets in size, or the
/// Provisioning procedure will fail and be canceled. If input type
/// is "in-alpha", the printable characters should be
/// left-justified, with trailing 0x00 octets filling the remaining
/// bytes.
pub type PromptStaticFn =
    Box<dyn (Fn(StaticCapability) -> Pin<Box<dyn Future<Output = ReqResult<[u8; 16]>> + Send>>) + Send + Sync>;

/// Arguments for display numeric function.
#[derive(Debug)]
#[non_exhaustive]
pub struct DisplayNumeric {
    /// Type of a display.
    pub display_type: NumericCapability,
    /// The value to display.
    pub number: u32,
}

/// Function handling displaying numeric values.
pub type DisplayNumericFn =
    Box<dyn (Fn(DisplayNumeric) -> Pin<Box<dyn Future<Output = ReqResult<()>> + Send>>) + Send + Sync>;

/// Mesh provision agent configuration.
#[derive(Default)]
pub struct ProvisionAgent {
    /// This method is called when the Daemon has something important
    /// for the Agent to Display, but does not require any additional
    /// input locally.
    ///
    /// For instance: "Enter 14939264 on remote device".
    pub display_numeric: Option<DisplayNumericFn>,

    /// This method is called when the Daemon requires a 16 octet byte
    /// array, as an Out-of-Band authentication.
    pub prompt_static: Option<PromptStaticFn>,

    /// Capabilities of provisioning agent.
    ///
    /// Default is empty, meaning no method will be used for provisioning
    pub capabilities: Vec<Capability>,

    #[doc(hidden)]
    pub _non_exhaustive: (),
}

impl fmt::Debug for ProvisionAgent {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("ProvisionAgent").finish()
    }
}

/// Implements org.bluez.mesh.ProvisionAgent1 interface
pub(crate) struct RegisteredProvisionAgent {
    agent: ProvisionAgent,
    inner: Arc<SessionInner>,
}

impl RegisteredProvisionAgent {
    pub(crate) fn new(agent: ProvisionAgent, inner: Arc<SessionInner>) -> Self {
        Self { agent, inner }
    }

    async fn call<A, F, R>(&self, f: &Option<impl Fn(A) -> F>, arg: A) -> ReqResult<R>
    where
        F: Future<Output = ReqResult<R>> + 'static,
    {
        match f {
            Some(f) => f(arg).await,
            None => Err(ReqError::Rejected),
        }
    }
}

#[zbus::interface(name = "org.bluez.mesh.ProvisionAgent1")]
impl RegisteredProvisionAgent {
    async fn display_numeric(&self, type_: String, value: u32) -> zbus::fdo::Result<()> {
        self.call(
            &self.agent.display_numeric,
            DisplayNumeric {
                display_type: NumericCapability::from_str(&type_).unwrap(),
                number: value,
            },
        )
        .await
        .map_err(zbus::fdo::Error::from)
    }

    async fn prompt_static(&self, type_: String) -> zbus::fdo::Result<Vec<u8>> {
        let data = self
            .call(&self.agent.prompt_static, StaticCapability::from_str(&type_).unwrap())
            .await
            .map_err(zbus::fdo::Error::from)?;
        Ok(Vec::from(data))
    }

    #[zbus(property)]
    fn capabilities(&self) -> Vec<String> {
        self.agent.capabilities.iter().map(|c| c.to_string()).collect()
    }
}
