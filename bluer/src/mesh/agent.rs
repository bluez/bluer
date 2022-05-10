//! Implement Provisioner bluetooth provisoner agent

use crate::{mesh::ReqError, method_call, SessionInner};
use std::sync::Arc;

use dbus::nonblock::{Proxy, SyncConnection};
use dbus_crossroads::{Crossroads, IfaceBuilder, IfaceToken};
use hex::FromHex;

use crate::mesh::{PATH, SERVICE_NAME, TIMEOUT};
use std::io::stdin;

pub(crate) const INTERFACE: &str = "org.bluez.mesh.ProvisionAgent1";

/// Provision agent configuration
#[derive(Clone, Default)]
pub struct ProvisionAgent {
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

    dbus_interface!();
    dbus_default_interface!(INTERFACE);

    pub(crate) fn register_interface(cr: &mut Crossroads) -> IfaceToken<Arc<Self>> {
        cr.register(INTERFACE, |ib: &mut IfaceBuilder<Arc<Self>>| {
            ib.method_with_cr_async(
                "DisplayNumeric",
                ("type", "value"),
                (),
                |ctx, cr, (_type, value): (String, u32)| {
                    method_call(ctx, cr, move |_reg: Arc<Self>| async move {
                        println!("Enter '{:?}' on the remote device!", value);
                        Ok(())
                    })
                },
            );
            ib.method_with_cr_async("PromptStatic", ("type",), ("value",), |ctx, cr, (_type,): (String,)| {
                method_call(ctx, cr, move |_reg: Arc<Self>| async move {
                    println!("Please input the value displayed on the device that is beaing provisioned: ");
                    let mut input_string = String::new();
                    stdin().read_line(&mut input_string).ok().expect("Failed to read input!");
                    let hex = Vec::from_hex(input_string.trim()).map_err(|_| ReqError::Failed)?;
                    Ok((hex,))
                })
            });
            cr_property!(ib, "Capabilities", reg => {
                Some(reg.agent.capabilities.clone())
            });
        })
    }
}
