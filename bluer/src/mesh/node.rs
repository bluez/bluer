//! Implements Node bluetooth mesh interface

use std::{collections::HashMap, sync::Arc};
use zbus::{
    proxy,
    zvariant::{OwnedObjectPath, OwnedValue},
};

use super::{
    application::ApplicationInner,
    element::{ElementConfigs, ElementRef},
};
use crate::{
    mesh::{management::Management, SERVICE_NAME},
    Result, SessionInner,
};

// pub(crate) const INTERFACE: &str = "org.bluez.mesh.Node1";

#[proxy(interface = "org.bluez.mesh.Node1")]
trait Node {
    /// Publish.
    fn publish(
        &self, element_path: &zbus::zvariant::ObjectPath<'_>, model_id: u16,
        options: HashMap<String, OwnedValue>, data: Vec<u8>,
    ) -> zbus::Result<()>;
    /// Send.
    fn send(
        &self, element_path: &zbus::zvariant::ObjectPath<'_>, destination: u16, key_index: u16,
        options: HashMap<String, OwnedValue>, data: Vec<u8>,
    ) -> zbus::Result<()>;
    /// DevKeySend.
    fn dev_key_send(
        &self, element_path: &zbus::zvariant::ObjectPath<'_>, destination: u16, remote: bool, net_index: u16,
        options: HashMap<String, OwnedValue>, data: Vec<u8>,
    ) -> zbus::Result<()>;
    /// AddAppKey.
    fn add_app_key(
        &self, element_path: &zbus::zvariant::ObjectPath<'_>, destination: u16, app_key_index: u16,
        net_key_index: u16, update: bool,
    ) -> zbus::Result<()>;
}

/// Interface to a Bluetooth mesh node.
#[derive(Clone)]
pub struct Node {
    inner: Arc<SessionInner>,
    app_inner: Arc<ApplicationInner>,
    path: OwnedObjectPath,
    // TODO: translate element_config into proper Rust type
    _element_config: Arc<ElementConfigs>,
}

impl Node {
    pub(crate) async fn new(
        inner: Arc<SessionInner>, app_inner: Arc<ApplicationInner>, path: OwnedObjectPath,
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
        let options = HashMap::new();

        log::trace!(
            "Publishing message: path={:?} model_id={:?} options={:?} data={:?}",
            &path,
            model_id,
            &options,
            data
        );

        let proxy = NodeProxy::builder(&self.inner.connection)
            .destination(SERVICE_NAME)?
            .path(self.path.clone())?
            .build()
            .await?;

        proxy.publish(&path, model_id, options, data.to_vec()).await.map_err(Into::into)
    }

    /// Send a message originated by a local model.
    pub async fn send(
        &self, element_ref: &ElementRef, destination: u16, key_index: u16, data: &[u8],
    ) -> Result<()> {
        let path = element_ref.path()?;
        let options = HashMap::new();

        log::trace!(
            "Sending message: path={:?} destination={:?} key_index={:?} options={:?} data={:?}",
            &path,
            destination,
            key_index,
            &options,
            data
        );

        let proxy = NodeProxy::builder(&self.inner.connection)
            .destination(SERVICE_NAME)?
            .path(self.path.clone())?
            .build()
            .await?;

        proxy.send(&path, destination, key_index, options, data.to_vec()).await.map_err(Into::into)
    }

    /// Send a message originated by a local model encoded with the device key of the remote node.
    pub async fn dev_key_send(
        &self, element_ref: &ElementRef, destination: u16, remote: bool, net_index: u16, data: &[u8],
    ) -> Result<()> {
        let path = element_ref.path()?;
        let options = HashMap::new();

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

        let proxy = NodeProxy::builder(&self.inner.connection)
            .destination(SERVICE_NAME)?
            .path(self.path.clone())?
            .build()
            .await?;

        proxy
            .dev_key_send(&path, destination, remote, net_index, options, data.to_vec())
            .await
            .map_err(Into::into)
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

        let proxy = NodeProxy::builder(&self.inner.connection)
            .destination(SERVICE_NAME)?
            .path(self.path.clone())?
            .build()
            .await?;

        proxy.add_app_key(&path, destination, app_key, net_index, update).await.map_err(Into::into)
    }
}
