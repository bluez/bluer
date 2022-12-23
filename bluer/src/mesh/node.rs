//! Implements Node bluetooth mesh interface

use crate::{InvalidAddress, Result, SessionInner};
use std::{collections::HashMap, sync::Arc};

use btmesh_common::ModelIdentifier;
use dbus::{
    arg::{RefArg, Variant},
    nonblock::{Proxy, SyncConnection},
    Path,
};

use crate::{
    mesh::{management::Management, SERVICE_NAME, TIMEOUT},
    Error, ErrorKind,
};
use btmesh_models::{
    foundation::configuration::{
        model_app::{ModelAppMessage, ModelAppPayload},
        model_publication::{
            ModelPublicationMessage, ModelPublicationSetMessage, PublicationDetails, PublishAddress,
            PublishPeriod, PublishRetransmit,
        },
        node_reset::NodeResetMessage,
        AppKeyIndex, ConfigurationMessage,
    },
    Message, Model,
};

use super::element::ElementControlHandle;

pub(crate) const INTERFACE: &str = "org.bluez.mesh.Node1";

/// Interface to a Bluetooth mesh node.
#[derive(Clone)]
pub struct Node {
    inner: Arc<SessionInner>,
    path: Path<'static>,
    /// Management interface for the node
    pub management: Management,
}

impl Node {
    pub(crate) async fn new(path: Path<'static>, inner: Arc<SessionInner>) -> Result<Self> {
        let management = Management::new(path.clone(), inner.clone()).await?;
        Ok(Self { inner, path, management })
    }

    /// Publish message to the mesh
    pub async fn publish<'m, M: Model>(&self, message: M::Message, path: Path<'m>) -> Result<()> {
        let model_id = match M::IDENTIFIER {
            ModelIdentifier::SIG(id) => id,
            ModelIdentifier::Vendor(_, id) => id,
        };

        let mut data: heapless::Vec<u8, 384> = heapless::Vec::new();
        message.opcode().emit(&mut data).map_err(|_| Error::new(ErrorKind::Failed))?;
        message.emit_parameters(&mut data).map_err(|_| Error::new(ErrorKind::Failed))?;

        let options: HashMap<&'static str, Variant<Box<dyn RefArg>>> = HashMap::new();

        log::trace!("Publishing message: {:?} {:?} {:?} {:?}", path, model_id, options, data.to_vec());
        self.call_method("Publish", (path, model_id, options, data.to_vec())).await?;

        Ok(())
    }

    /// Send a publication originated by a local model.
    pub async fn send<'m, M: Message>(
        &self, message: &M, element: ElementControlHandle, destination: u16, app_key: u16,
    ) -> Result<()> {
        let path = element.path.ok_or(Error::new(ErrorKind::Failed))?;
        let mut data: heapless::Vec<u8, 384> = heapless::Vec::new();
        message.opcode().emit(&mut data).map_err(|_| Error::new(ErrorKind::Failed))?;
        message.emit_parameters(&mut data).map_err(|_| Error::new(ErrorKind::Failed))?;

        let options: HashMap<&'static str, Variant<Box<dyn RefArg>>> = HashMap::new();

        log::trace!(
            "Sending message: {:?} {:?} {:?} {:?} {:?}",
            path,
            destination,
            app_key,
            options,
            data.to_vec()
        );
        self.call_method("Send", (path, destination, app_key, options, data.to_vec())).await?;
        Ok(())
    }

    /// Send a message originated by a local model encoded with the device key of the remote node.
    pub async fn dev_key_send<'m, M: Message>(
        &self, message: &M, path: Path<'m>, destination: u16, remote: bool, app_key: u16,
    ) -> Result<()> {
        let mut data: heapless::Vec<u8, 384> = heapless::Vec::new();
        message.opcode().emit(&mut data).map_err(|_| Error::new(ErrorKind::Failed))?;
        message.emit_parameters(&mut data).map_err(|_| Error::new(ErrorKind::Failed))?;

        let options: HashMap<&'static str, Variant<Box<dyn RefArg>>> = HashMap::new();

        log::trace!(
            "Sending device key encoded message: {:?} {:?} {:?} {:?} {:?} {:?}",
            path,
            destination,
            remote,
            app_key,
            options,
            data.to_vec()
        );
        self.call_method("DevKeySend", (path, destination, remote, app_key, options, data.to_vec())).await?;
        Ok(())
    }

    /// Send add or update network key originated by the local configuration client to a remote configuration server.
    pub async fn add_app_key<'m>(
        &self, path: Path<'m>, destination: u16, app_key: u16, net_key: u16, update: bool,
    ) -> Result<()> {
        log::trace!("Adding app key: {:?} {:?} {:?} {:?} {:?}", path, destination, app_key, net_key, update);
        self.call_method("AddAppKey", (path, destination, app_key, net_key, update)).await?;
        Ok(())
    }

    /// Create bind configuration message
    pub fn bind_create<'m>(address: u16, app_key: u16, model: ModelIdentifier) -> Result<ConfigurationMessage> {
        let payload = ModelAppPayload {
            element_address: address.try_into().map_err(|_| InvalidAddress(address.to_string()))?,
            app_key_index: AppKeyIndex::new(app_key),
            model_identifier: model,
        };

        Ok(ConfigurationMessage::from(ModelAppMessage::Bind(payload)))
    }

    /// Binds application key to the model.
    pub async fn bind<'m>(
        &self, element_path: Path<'m>, address: u16, app_key: u16, model: ModelIdentifier,
    ) -> Result<()> {
        let message = Self::bind_create(address, app_key, model)?;
        self.dev_key_send(&message, element_path.clone(), address, true, 0 as u16).await?;
        Ok(())
    }

    /// Reset a node.
    pub async fn reset<'m>(&self, element_path: Path<'m>, address: u16) -> Result<()> {
        let message = ConfigurationMessage::from(NodeResetMessage::Reset);
        self.dev_key_send(&message, element_path.clone(), address, true, 0 as u16).await?;
        Ok(())
    }

    /// Create pub-set configuration message
    pub fn pub_set_create<'m>( address: u16, pub_address: PublishAddress, app_key: u16, publish_period: PublishPeriod, rxt: PublishRetransmit, model: ModelIdentifier) -> Result<ConfigurationMessage> {
        let details = PublicationDetails {
            element_address: address.try_into().map_err(|_| InvalidAddress(address.to_string()))?,
            publish_address: pub_address,
            app_key_index: AppKeyIndex::new(app_key),
            credential_flag: false,
            publish_ttl: None,
            publish_period: publish_period,
            publish_retransmit: rxt,
            model_identifier: model,
        };

        let set = ModelPublicationSetMessage { details };

        Ok(ConfigurationMessage::from(ModelPublicationMessage::VirtualAddressSet(set)))
    }

    /// Sets publication to the model.
    pub async fn pub_set<'m>(
        &self, element_path: Path<'m>, address: u16, pub_address: PublishAddress, app_key: u16,
        publish_period: PublishPeriod, rxt: PublishRetransmit, model: ModelIdentifier,
    ) -> Result<()> {
        let message = Self::pub_set_create(address, pub_address, app_key, publish_period, rxt, model)?;
        self.dev_key_send(&message, element_path.clone(), address, true, 0 as u16).await?;
        Ok(())
    }

    fn proxy(&self) -> Proxy<'_, &SyncConnection> {
        Proxy::new(SERVICE_NAME, self.path.clone(), TIMEOUT, &*self.inner.connection)
    }

    dbus_interface!();
    dbus_default_interface!(INTERFACE);
}
