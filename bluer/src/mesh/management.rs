//! Implements Node bluetooth mesh interface

use crate::{Result, SessionInner};
use std::{collections::HashMap, sync::Arc};

use dbus::{
    arg::{RefArg, Variant},
    nonblock::{Proxy, SyncConnection},
    Path,
};
use uuid::Uuid;

use crate::mesh::{SERVICE_NAME, TIMEOUT};

pub(crate) const INTERFACE: &str = "org.bluez.mesh.Management1";

/// Interface to a Bluetooth mesh node.
#[derive(Clone)]
pub struct Management {
    inner: Arc<SessionInner>,
    path: Path<'static>,
}

impl Management {
    pub(crate) async fn new(path: Path<'static>, inner: Arc<SessionInner>) -> Result<Self> {
        Ok(Self { inner, path })
    }

    /// Publish message to the mesh
    pub async fn add_node(&self, uuid: Uuid) -> Result<()> {
        self.call_method(
            "AddNode",
            (uuid.as_bytes().to_vec(), HashMap::<String, Variant<Box<dyn RefArg + 'static>>>::new()),
        )
        .await?;

        Ok(())
    }

    fn proxy(&self) -> Proxy<'_, &SyncConnection> {
        Proxy::new(SERVICE_NAME, self.path.clone(), TIMEOUT, &*self.inner.connection)
    }

    dbus_interface!();
    dbus_default_interface!(INTERFACE);
}
