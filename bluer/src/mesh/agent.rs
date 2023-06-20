//! Bluetooth mesh provisoner agent.

use core::fmt;
use dbus::nonblock::{Proxy, SyncConnection};
use dbus_crossroads::{Crossroads, IfaceBuilder, IfaceToken};
use futures::Future;
use std::{fmt::Debug, pin::Pin, str::FromStr, sync::Arc};
use strum::{EnumString, IntoStaticStr};

use crate::{
    mesh::{PATH, SERVICE_NAME, TIMEOUT},
    method_call, SessionInner, ERR_PREFIX,
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

impl From<ReqError> for dbus::MethodErr {
    fn from(err: ReqError) -> Self {
        let name: &'static str = err.into();
        Self::from((ERR_PREFIX.to_string() + name, &err.to_string()))
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

    fn proxy(&self) -> Proxy<'_, &SyncConnection> {
        Proxy::new(SERVICE_NAME, PATH, TIMEOUT, &*self.inner.connection)
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

    dbus_interface!();
    dbus_default_interface!(INTERFACE);

    pub(crate) fn register_interface(cr: &mut Crossroads) -> IfaceToken<Arc<Self>> {
        cr.register(INTERFACE, |ib: &mut IfaceBuilder<Arc<Self>>| {
            ib.method_with_cr_async(
                "DisplayNumeric",
                ("type", "value"),
                (),
                |ctx, cr, (display_type, number): (String, u32)| {
                    method_call(ctx, cr, move |reg: Arc<Self>| async move {
                        reg.call(
                            &reg.agent.display_numeric,
                            DisplayNumeric {
                                display_type: NumericCapability::from_str(&display_type).unwrap(),
                                number,
                            },
                        )
                        .await?;
                        Ok(())
                    })
                },
            );
            ib.method_with_cr_async(
                "PromptStatic",
                ("type",),
                ("value",),
                |ctx, cr, (input_type,): (String,)| {
                    method_call(ctx, cr, move |reg: Arc<Self>| async move {
                        let data = reg
                            .call(&reg.agent.prompt_static, StaticCapability::from_str(&input_type).unwrap())
                            .await?;
                        Ok((Vec::from(data),))
                    })
                },
            );

            cr_property!(ib, "Capabilities", reg => {
                Some(reg.agent.capabilities.iter().map(|c| c.to_string()).collect::<Vec<_>>())
            });
        })
    }
}
