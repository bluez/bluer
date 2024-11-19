//! Implement Network bluetooth mesh interface

use dbus::{
    nonblock::{Proxy, SyncConnection},
    Path,
};
use std::sync::Arc;
use tokio::sync::oneshot;

use crate::{
    mesh::{
        application::{Application, ApplicationHandle, RegisteredApplication},
        element::ElementConfig,
        node::Node,
        PATH, SERVICE_NAME, TIMEOUT,
    },
    Error, ErrorKind, Result, SessionInner,
};

pub(crate) const INTERFACE: &str = "org.bluez.mesh.Network1";

/// Interface to a Bluetooth mesh network.
///
/// Use [`Session::mesh`](crate::Session::mesh) to obtain an instance.
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
        RegisteredApplication::register(self.inner.clone(), app).await
    }

    /// Join mesh network.
    ///
    /// This is the first method that an application has to call to
    /// become a provisioned node on a mesh network. The call will
    /// initiate broadcasting of Unprovisioned Device Beacon.
    ///
    /// The application UUID must be unique (at least from the daemon perspective),
    /// therefore attempting to call this function using already
    /// registered UUID results in an error. The composition of the UUID
    /// octets must be in compliance with RFC 4122.
    pub async fn join(&self, app: Application) -> Result<ApplicationHandle> {
        let mut app_hnd = self.application(app).await?;

        let (done_tx, done_rx) = oneshot::channel();
        let connection = self.inner.connection.clone();
        tokio::spawn(async move {
            if done_rx.await.is_err() {
                let proxy = Proxy::new(SERVICE_NAME, PATH, TIMEOUT, &*connection);
                let _: std::result::Result<(), dbus::Error> = proxy.method_call(INTERFACE, "Cancel", ()).await;
            }
        });

        let () = self.call_method("Join", (app_hnd.name.clone(), app_hnd.device_id.as_bytes().to_vec())).await?;

        let result = match app_hnd.join_result_rx.recv().await {
            Some(Ok(token)) => {
                app_hnd.token = Some(token);
                Ok(app_hnd)
            }
            Some(Err(reason)) => Err(reason.into()),
            None => Err(Error::new(ErrorKind::Failed)),
        };
        let _ = done_tx.send(());
        result
    }

    /// Attach to mesh network.
    ///
    /// This is the first method that an application must call to get
    /// access to mesh node functionalities.
    ///
    /// The token parameter is a 64-bit number that has been assigned to
    /// the application when it first got provisioned/joined mesh
    /// network.
    /// The daemon uses the token to verify whether the application is authorized
    /// to assume the mesh node identity.
    pub async fn attach(&self, app: Application, token: u64) -> Result<Node> {
        let app_hnd = self.application(app).await?;

        #[allow(clippy::type_complexity)]
        let (node_path, element_config): (Path<'static>, Vec<(u8, Vec<(u16, ElementConfig)>)>) =
            self.call_method("Attach", (app_hnd.name.clone(), token)).await?;
        let element_config =
            element_config.into_iter().map(|(idx, ent)| (idx as usize, ent.into_iter().collect())).collect();

        log::debug!("Attached mesh app to {:?} with elements config {:?}", node_path, &element_config);

        Node::new(self.inner.clone(), app_hnd.app_inner.clone(), node_path.clone(), element_config).await
    }

    /// Leave mesh network.
    ///
    /// This removes the configuration information about the mesh node
    /// identified by the 64-bit token parameter.
    pub async fn leave(&self, token: u64) -> Result<()> {
        self.call_method("Leave", (token,)).await
    }

    dbus_interface!();
    dbus_default_interface!(INTERFACE);
}
