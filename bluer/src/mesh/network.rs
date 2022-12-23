//! Implement Network bluetooth mesh interface

use crate::{Error, ErrorKind, InternalErrorKind, Result, SessionInner};
use std::sync::Arc;

use dbus::{
    nonblock::{Proxy, SyncConnection},
    Path,
};

use crate::mesh::{
    all_dbus_objects,
    application::{Application, ApplicationHandle, RegisteredApplication},
    element::ElementConfig,
    node::Node,
    PATH, SERVICE_NAME, TIMEOUT,
};
use uuid::Uuid;

pub(crate) const INTERFACE: &str = "org.bluez.mesh.Network1";

/// Interface to a Bluetooth mesh network.
#[derive(Clone)]
pub struct Network {
    inner: Arc<SessionInner>,
}

impl Network {
    pub(crate) async fn new(inner: Arc<SessionInner>) -> Result<Self> {
        Ok(Self { inner })
    }

    fn proxy(&self) -> Proxy<'_, &SyncConnection> {
        Proxy::new(SERVICE_NAME, PATH, TIMEOUT, &*self.inner.connection)
    }

    /// Create mesh application
    async fn application(&self, app: Application) -> Result<ApplicationHandle> {
        let reg = RegisteredApplication::new(self.inner.clone(), app);

        reg.register(self.inner.clone()).await
    }

    /// Join mesh network
    pub async fn join(&self, app: Application, uuid: Uuid) -> Result<ApplicationHandle> {
        let handle = self.application(app).await?;

        self.call_method("Join", (handle.name.clone(), uuid.as_bytes().to_vec())).await?;

        Ok(handle)
    }

    /// Attach to mesh network
    pub async fn attach(&self, app: Application, token: &str) -> Result<(ApplicationHandle, Node)> {
        let token_int = u64::from_str_radix(token, 16)
            .map_err(|_| Error::new(ErrorKind::Internal(InternalErrorKind::InvalidValue)))?;

        let handle = self.application(app).await?;

        let (node_path, config): (Path<'static>, Vec<(u8, Vec<(u16, ElementConfig)>)>) =
            self.call_method("Attach", (handle.name.clone(), token_int)).await?;

        log::info!("Attached app to {:?} with elements config {:?}", node_path, config);

        let node = Node::new(node_path.clone(), self.inner.clone()).await?;

        Ok((handle, node))
    }

    /// Cancel provisioning request
    pub async fn cancel(&self) -> Result<()> {
        self.call_method("Cancel", ()).await
    }

    /// Leave mesh network
    pub async fn leave(&self, token: &str) -> Result<()> {
        let token_int = u64::from_str_radix(token, 16)
            .map_err(|_| Error::new(ErrorKind::Internal(InternalErrorKind::InvalidValue)))?;

        self.call_method("Leave", (token_int,)).await
    }

    /// Temporary debug method to print the state of mesh
    pub async fn print_dbus_objects(&self) -> Result<()> {
        for (path, interfaces) in all_dbus_objects(&*self.inner.connection).await? {
            println!("{}", path);
            for (interface, _props) in interfaces {
                println!("    - interface {}", interface);
            }
        }
        Ok(())
    }

    dbus_interface!();
    dbus_default_interface!(INTERFACE);
}
