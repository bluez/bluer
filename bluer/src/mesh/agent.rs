//! Implement Provisioner bluetooth provisoner agent

use crate::{method_call, SessionInner, ERR_PREFIX};
use futures::Future;
use std::{pin::Pin, sync::Arc};

use dbus::nonblock::{Proxy, SyncConnection};
use dbus_crossroads::{Crossroads, IfaceBuilder, IfaceToken};
use std::str::FromStr;

use crate::mesh::{PATH, SERVICE_NAME, TIMEOUT};
use strum::{EnumString, IntoStaticStr};

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
#[derive(Debug, PartialEq, EnumString)]
pub enum StaticCapabilities {
    /// 16 octet alpha array.
    #[strum(serialize = "in-alpha")]
    InAlpha,
    /// 16 octet array.
    #[strum(serialize = "static-oob")]
    StaticOob,
}

/// Agent numeric capabilities.
#[derive(Debug, PartialEq, EnumString)]
pub enum NumericCapabilities {
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

/// Function handling a static OOB authentication.
pub type PromptStaticFn = fn(StaticCapabilities) -> Pin<Box<dyn Future<Output = ReqResult<Vec<u8>>> + Send>>;

/// Arguments for display numeric function.
#[derive(Debug)]
pub struct DisplayNumeric {
    /// Type of a display.
    pub display_type: NumericCapabilities,
    /// The value to display.
    pub number: u32,
}

/// Function handling displaying numeric values.
pub type DisplayNumericFn = fn(DisplayNumeric) -> Pin<Box<dyn Future<Output = ReqResult<()>> + Send>>;

/// Provision agent configuration.
#[derive(Clone, Default)]
pub struct ProvisionAgent {
    display_numeric: Option<DisplayNumericFn>,
    prompt_static: Option<PromptStaticFn>,

    /// Capabilities of provisioning agent.
    /// Default is empty, meaning no method will be used for provisioning
    /// Change it with something like, vec!["out-numeric".into(), "static-oob".into()]
    capabilities: Vec<String>,
}

#[derive(Clone)]
/// Implements org.bluez.mesh.ProvisionAgent1 interface
pub struct RegisteredProvisionAgent {
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
                        let res = reg
                            .call(
                                &reg.agent.display_numeric,
                                DisplayNumeric {
                                    display_type: NumericCapabilities::from_str(&display_type).unwrap(),
                                    number,
                                },
                            )
                            .await?;
                        Ok(res)
                    })
                },
            );
            ib.method_with_cr_async(
                "PromptStatic",
                ("type",),
                ("value",),
                |ctx, cr, (input_type,): (String,)| {
                    method_call(ctx, cr, move |reg: Arc<Self>| async move {
                        let hex = reg
                            .call(&reg.agent.prompt_static, StaticCapabilities::from_str(&input_type).unwrap())
                            .await?;
                        Ok((hex,))
                    })
                },
            );
            cr_property!(ib, "Capabilities", reg => {
                Some(reg.agent.capabilities.clone())
            });
        })
    }
}
