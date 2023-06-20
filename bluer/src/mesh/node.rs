//! Implements Node bluetooth mesh interface

use dbus::{
    arg::{RefArg, Variant},
    nonblock::{Proxy, SyncConnection},
    Path,
};
use std::{collections::HashMap, sync::Arc};

use super::{
    application::ApplicationInner,
    element::{ElementConfigs, ElementRef},
};
use crate::{
    mesh::{management::Management, SERVICE_NAME, TIMEOUT},
    Result, SessionInner,
};

pub(crate) const INTERFACE: &str = "org.bluez.mesh.Node1";

/// Interface to a Bluetooth mesh node.
#[derive(Clone)]
pub struct Node {
    inner: Arc<SessionInner>,
    app_inner: Arc<ApplicationInner>,
    path: Path<'static>,
    // TODO: translate element_config into proper Rust type
    _element_config: Arc<ElementConfigs>,
}

impl Node {
    pub(crate) async fn new(
        inner: Arc<SessionInner>, app_inner: Arc<ApplicationInner>, path: Path<'static>,
        element_config: ElementConfigs,
    ) -> Result<Self> {
        Ok(Self { inner, app_inner, path, _element_config: Arc::new(element_config) })
    }

    /// Management interface for the node.
    pub fn management(&self) -> Management {
        Management::new(self.inner.clone(), self.app_inner.clone(), self.path.clone())
    }

    /// Send a publication originated by a local model.
    ///
    /// Since only one Publish record may exist per element-model, the
    /// destination and key_index are obtained from the Publication
    /// record cached by the daemon.
    pub async fn publish(&self, element_ref: &ElementRef, model_id: u16, data: &[u8]) -> Result<()> {
        let path = element_ref.path()?;
        let options: HashMap<&'static str, Variant<Box<dyn RefArg>>> = HashMap::new();

        log::trace!(
            "Publishing message: path={:?} model_id={:?} options={:?} data={:?}",
            &path,
            model_id,
            &options,
            data
        );
        self.call_method("Publish", (path, model_id, options, data.to_vec())).await?;

        Ok(())
    }

    /// Send a message originated by a local model.
    pub async fn send(
        &self, element_ref: &ElementRef, destination: u16, key_index: u16, data: &[u8],
    ) -> Result<()> {
        let path = element_ref.path()?;
        let options: HashMap<&'static str, Variant<Box<dyn RefArg>>> = HashMap::new();

        log::trace!(
            "Sending message: path={:?} destination={:?} key_index={:?} options={:?} data={:?}",
            &path,
            destination,
            key_index,
            &options,
            data
        );
        self.call_method("Send", (path, destination, key_index, options, data.to_vec())).await?;

        Ok(())
    }

    /// Send a message originated by a local model encoded with the device key of the remote node.
    pub async fn dev_key_send(
        &self, element_ref: &ElementRef, destination: u16, remote: bool, net_index: u16, data: &[u8],
    ) -> Result<()> {
        let path = element_ref.path()?;
        let options: HashMap<&'static str, Variant<Box<dyn RefArg>>> = HashMap::new();

        log::trace!(
            "Sending device key encoded message: path={:?} destination={:?} remote={:?} net_index={:?} options={:?} \
            data={:?}",
            &path,
            destination,
            remote,
            net_index,
            &options,
            data
        );
        self.call_method("DevKeySend", (path, destination, remote, net_index, options, data.to_vec())).await?;

        Ok(())
    }

    /// Send add or update network key originated by the local configuration client to a remote configuration server.
    pub async fn add_app_key(
        &self, element_ref: &ElementRef, destination: u16, app_key: u16, net_index: u16, update: bool,
    ) -> Result<()> {
        let path = element_ref.path()?;

        log::trace!(
            "Adding app key: path={:?} destination={:?} app_key={:?} net_index={:?} update={:?}",
            path,
            destination,
            app_key,
            net_index,
            update
        );
        self.call_method("AddAppKey", (path, destination, app_key, net_index, update)).await?;

        Ok(())
    }

    fn proxy(&self) -> Proxy<'_, &SyncConnection> {
        Proxy::new(SERVICE_NAME, self.path.clone(), TIMEOUT, &*self.inner.connection)
    }

    dbus_interface!();
    dbus_default_interface!(INTERFACE);
}
