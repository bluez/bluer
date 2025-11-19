//! Bluetooth session.

use zbus::{
    Connection,
    // export::ordered_stream::OrderedStreamExt,
    fdo::ObjectManagerProxy,
};
// use dbus::{
//     arg::Variant,
//     message::MatchRule,
//     nonblock::{
//         stdintf::org_freedesktop_dbus::{
//             ObjectManagerInterfacesAdded, ObjectManagerInterfacesRemoved, PropertiesPropertiesChanged,
//         },
//         SyncConnection,
//     },
//     strings::BusName,
//     Message,
// };
// use dbus_crossroads::{Crossroads, IfaceToken};
// use dbus_tokio::connection;
use futures::{
    channel::{mpsc, oneshot},
    lock::Mutex,
    Future, SinkExt, Stream, StreamExt,
};
use lazy_static::lazy_static;
use std::{
    collections::{HashMap, HashSet},
    fmt::{Debug, Formatter},
    sync::{Arc, Weak},
};
use tokio::{
    select,
    task::{spawn_blocking, JoinHandle},
};

use crate::{
    // adapter,
    // adv::Advertisement,
    // agent::{Agent, AgentHandle, RegisteredAgent},
    // all_dbus_objects, gatt,
    // monitor::RegisteredMonitor,
    // parent_path, Adapter, DiscoveryFilter, Error, ErrorKind, InternalErrorKind, Result, SERVICE_NAME,
    Error, ErrorKind, InternalErrorKind, Result, SERVICE_NAME, Adapter,
};

// TODO: re-enable when mesh module is ported
// #[cfg(feature = "mesh")]
// use crate::mesh::{
//     agent::RegisteredProvisionAgent, application::RegisteredApplication, element::RegisteredElement,
//     network::Network, provisioner::RegisteredProvisioner,
// };

// TODO: re-enable when rfcomm module is ported
// #[cfg(feature = "rfcomm")]
// use crate::rfcomm::{profile::RegisteredProfile, Profile, ProfileHandle};

/// Terminate TX and terminated RX for single session.
type SingleSessionTerm = (Weak<oneshot::Sender<()>>, oneshot::Receiver<()>);

/// Shared state of all objects in a Bluetooth session.
pub(crate) struct SessionInner {
    pub connection: Connection,
    // pub crossroads: Mutex<Crossroads>,
    // pub le_advertisment_token: IfaceToken<Advertisement>,
    // pub gatt_reg_service_token: IfaceToken<Arc<gatt::local::RegisteredService>>,
    // pub gatt_reg_characteristic_token: IfaceToken<Arc<gatt::local::RegisteredCharacteristic>>,
    // pub gatt_reg_characteristic_descriptor_token: IfaceToken<Arc<gatt::local::RegisteredDescriptor>>,
    // pub gatt_profile_token: IfaceToken<gatt::local::Profile>,
    // pub agent_token: IfaceToken<Arc<RegisteredAgent>>,
    // #[cfg(feature = "mesh")]
    // pub application_token: IfaceToken<Arc<RegisteredApplication>>,
    // #[cfg(feature = "mesh")]
    // pub element_token: IfaceToken<Arc<RegisteredElement>>,
    // #[cfg(feature = "mesh")]
    // pub provisioner_token: IfaceToken<Arc<RegisteredApplication>>,
    // #[cfg(feature = "mesh")]
    // pub provision_agent_token: IfaceToken<Arc<RegisteredProvisionAgent>>,
    // pub monitor_token: IfaceToken<Arc<RegisteredMonitor>>,
    // #[cfg(feature = "rfcomm")]
    // pub profile_token: IfaceToken<Arc<RegisteredProfile>>,
    pub single_sessions: Mutex<HashMap<zbus::zvariant::OwnedObjectPath, SingleSessionTerm>>,
    pub event_sub_tx: mpsc::Sender<SubscriptionReq>,
    // dbus_task: JoinHandle<connection::IOResourceError>,
    pub adapter_discovery_filter: Mutex<HashMap<String, crate::DiscoveryFilter>>,
}

impl SessionInner {
    pub async fn single_session(
        &self, path: &zbus::zvariant::OwnedObjectPath, start_fn: impl Future<Output = Result<()>>,
        stop_fn: impl Future<Output = ()> + Send + 'static,
    ) -> Result<SingleSessionToken> {
        let mut single_sessions = self.single_sessions.lock().await;

        if let Some((term_tx_weak, termed_rx)) = single_sessions.get_mut(path) {
            match term_tx_weak.upgrade() {
                Some(term_tx) => {
                    log::trace!("Using existing single session for {}", &path);
                    return Ok(SingleSessionToken(term_tx));
                }
                None => {
                    log::trace!("Waiting for termination of previous single session for {}", &path);
                    let _ = termed_rx.await;
                    single_sessions.remove(path);
                }
            }
        }

        log::trace!("Starting new single session for {}", &path);
        start_fn.await?;

        let (term_tx, term_rx) = oneshot::channel();
        let term_tx = Arc::new(term_tx);
        let (termed_tx, termed_rx) = oneshot::channel();
        single_sessions.insert(path.clone(), (Arc::downgrade(&term_tx), termed_rx));

        let path = path.clone();
        tokio::spawn(async move {
            let _ = term_rx.await;
            log::trace!("Terminating single session for {}", &path);
            stop_fn.await;
            let _ = termed_tx.send(());
            log::trace!("Terminated single session for {}", &path);
        });

        Ok(SingleSessionToken(term_tx))
    }

    pub async fn is_single_session_active(&self, path: &zbus::zvariant::OwnedObjectPath) -> bool {
        let mut single_sessions = self.single_sessions.lock().await;

        if let Some((term_tx_weak, termed_rx)) = single_sessions.get_mut(path) {
            match term_tx_weak.upgrade() {
                Some(_) => true,
                None => {
                    log::trace!("Waiting for termination of previous single session for {}", &path);
                    let _ = termed_rx.await;
                    single_sessions.remove(path);
                    false
                }
            }
        } else {
            false
        }
    }

    pub async fn events(
        &self, path: zbus::zvariant::OwnedObjectPath, child_objects: bool,
    ) -> Result<mpsc::UnboundedReceiver<Event>> {
        Event::subscribe(&mut self.event_sub_tx.clone(), path, child_objects).await
    }
}

#[derive(Clone)]
#[allow(dead_code)]
pub(crate) struct SingleSessionToken(Arc<oneshot::Sender<()>>);

impl Drop for SingleSessionToken {
    fn drop(&mut self) {
        // required for drop order
    }
}

/// Bluetooth session.
///
/// Encapsulates a connection to the system Bluetooth daemon.
#[cfg_attr(docsrs, doc(cfg(feature = "bluetoothd")))]
#[derive(Clone)]
pub struct Session {
    inner: Arc<SessionInner>,
}

impl Debug for Session {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        write!(f, "Session {{ {:?} }}", self.inner.connection.unique_name())
    }
}

/// Bluetooth session event.
#[cfg_attr(docsrs, doc(cfg(feature = "bluetoothd")))]
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SessionEvent {
    /// Adapter added.
    AdapterAdded(String),
    /// Adapter removed.
    AdapterRemoved(String),
}

impl Session {
    /// Create a new Bluetooth session.
    ///
    /// This establishes a connection to the system Bluetooth daemon over D-Bus.
    pub async fn new() -> Result<Self> {
        let connection = Connection::system().await?;
        let (event_sub_tx, event_sub_rx) = mpsc::channel(1);
        Event::handle_connection(connection.clone(), event_sub_rx).await?;

        Ok(Self {
            inner: Arc::new(SessionInner {
                connection,
                // crossroads: Mutex::new(Crossroads::new()),
                // le_advertisment_token: IfaceToken::new(),
                // gatt_reg_service_token: IfaceToken::new(),
                // gatt_reg_characteristic_token: IfaceToken::new(),
                // gatt_reg_characteristic_descriptor_token: IfaceToken::new(),
                // gatt_profile_token: IfaceToken::new(),
                // agent_token: IfaceToken::new(),
                // #[cfg(feature = "mesh")]
                // application_token: IfaceToken::new(),
                // #[cfg(feature = "mesh")]
                // element_token: IfaceToken::new(),
                // #[cfg(feature = "mesh")]
                // provisioner_token: IfaceToken::new(),
                // #[cfg(feature = "mesh")]
                // provision_agent_token: IfaceToken::new(),
                // monitor_token: IfaceToken::new(),
                // #[cfg(feature = "rfcomm")]
                // profile_token: IfaceToken::new(),
                single_sessions: Mutex::new(HashMap::new()),
                event_sub_tx,
                // dbus_task,
                adapter_discovery_filter: Mutex::new(HashMap::new()),
            }),
        })
    }

    /// Streams session events.
    pub async fn events(&self) -> Result<impl Stream<Item = SessionEvent>> {
        let object_manager = ObjectManagerProxy::builder(&self.inner.connection)
            .destination(SERVICE_NAME)?
            .path("/")?
            .build()
            .await?;

        let added = object_manager.receive_interfaces_added().await?;
        let removed = object_manager.receive_interfaces_removed().await?;

        let added_stream = StreamExt::filter_map(added, |msg| async move {
            let args = msg.args().ok()?;
            let path = args.object_path;
            let interfaces = args.interfaces_and_properties;
            if interfaces.contains_key("org.bluez.Adapter1") {
                let name = path.split('/').last()?.to_string();
                Some(SessionEvent::AdapterAdded(name))
            } else {
                None
            }
        });

        let removed_stream = StreamExt::filter_map(removed, |msg| async move {
            let args = msg.args().ok()?;
            let path = args.object_path;
            let interfaces = args.interfaces;
            if interfaces.contains(&"org.bluez.Adapter1") {
                 let name = path.split('/').last()?.to_string();
                 Some(SessionEvent::AdapterRemoved(name))
            } else {
                None
            }
        });
        
        Ok(futures::stream::select(added_stream, removed_stream))
    }

    /// Get a list of available Bluetooth adapters.
    pub async fn adapter_names(&self) -> Result<Vec<String>> {
        let object_manager = ObjectManagerProxy::builder(&self.inner.connection)
            .destination(SERVICE_NAME)?
            .path("/")?
            .build()
            .await?;
        
        let objects = object_manager.get_managed_objects().await?;
        let mut names = Vec::new();
        for (path, interfaces) in objects {
            if interfaces.contains_key("org.bluez.Adapter1") {
                if let Some(name) = path.split('/').last() {
                    names.push(name.to_string());
                }
            }
        }
        Ok(names)
    }

    /// Get the default adapter.
    pub async fn default_adapter(&self) -> Result<Adapter> {
        let names = self.adapter_names().await?;
        if let Some(name) = names.first() {
            self.adapter(name)
        } else {
            Err(Error::new(ErrorKind::NotFound))
        }
    }

    /// Create an interface to the Bluetooth adapter with the specified name.
    pub fn adapter(&self, adapter_name: &str) -> Result<Adapter> {
        Adapter::new(self.inner.clone(), adapter_name)
    }

    // /// Create an interface for the Bluetooth mesh network.
    // TODO: re-enable when mesh module is ported
    // #[cfg(feature = "mesh")]
    // #[cfg_attr(docsrs, doc(cfg(feature = "mesh")))]
    // pub async fn mesh(&self) -> Result<Network> {
    //     Network::new(self.inner.clone()).await
    // }

    // TODO: re-enable when agent module is ported
    // /// Registers a Bluetooth authorization agent handler.
    // ///
    // /// Every application can register its own agent to use
    // /// that agent for all actions triggered by that application.
    // ///
    // /// It is not required by an application to register
    // /// an agent. If an application chooses not to
    // /// register an agent, the default agent is used. This
    // /// is in most cases a good idea. Only applications
    // /// like a pairing wizard should register their own
    // // agent.
    // //
    // // An application can only register one agent. Multiple
    // // agents per application are not supported.
    // //
    // // Drop the returned [AgentHandle] to unregister the agent.
    // pub async fn register_agent(&self, agent: Agent) -> Result<AgentHandle> {
    //     let reg_agent = RegisteredAgent::new(agent);
    //     reg_agent.register(self.inner.clone()).await
    // }

    // TODO: re-enable when rfcomm module is ported
// This registers a [Bluetooth profile implementation](Profile) for RFCOMM connections.
//
// The returned [ProfileHandle] provides a stream of
// [connection requests](crate::rfcomm::ConnectRequest).
//
// Drop the handle to unregister the profile.
    // #[cfg(feature = "rfcomm")]
    // #[cfg_attr(docsrs, doc(cfg(feature = "rfcomm")))]
    // pub async fn register_profile(&self, profile: Profile) -> Result<ProfileHandle> {
    //     let (req_tx, req_rx) = tokio::sync::mpsc::channel(1);
    //     let reg_profile = RegisteredProfile::new(req_tx);
    //     reg_profile.register(self.inner.clone(), profile, req_rx).await
    // }
}

/// A D-Bus object or property event.
#[derive(Debug, Clone)]
pub(crate) enum Event {
    /// Object or object interfaces added.
    ObjectAdded { object: zbus::zvariant::OwnedObjectPath, interfaces: HashSet<String> },
    /// Object or object interfaces removed.
    ObjectRemoved { object: zbus::zvariant::OwnedObjectPath, interfaces: HashSet<String> },
    /// Properties changed.
    PropertiesChanged { object: zbus::zvariant::OwnedObjectPath, interface: String, changed: Arc<HashMap<String, zbus::zvariant::OwnedValue>> },
}

/// D-Bus events subscription request.
pub(crate) struct SubscriptionReq {
    path: zbus::zvariant::OwnedObjectPath,
    child_objects: bool,
    tx: mpsc::UnboundedSender<Event>,
    ready_tx: oneshot::Sender<()>,
}

impl Event {
    pub(crate) async fn subscribe(
        tx: &mut mpsc::Sender<SubscriptionReq>,
        path: zbus::zvariant::OwnedObjectPath,
        child_objects: bool,
    ) -> Result<mpsc::UnboundedReceiver<Event>> {
        let (ready_tx, ready_rx) = oneshot::channel();
        let (event_tx, event_rx) = mpsc::unbounded();
        tx.send(SubscriptionReq {
            path,
            child_objects,
            tx: event_tx,
            ready_tx,
        }).await.map_err(|_| Error::new(ErrorKind::Internal(InternalErrorKind::Cancelled)))?;
        ready_rx.await.map_err(|_| Error::new(ErrorKind::Internal(InternalErrorKind::Cancelled)))?;
        Ok(event_rx)
    }

    /// Spawns a task that handles events for the specified connection.
    pub(crate) async fn handle_connection(
        connection: zbus::Connection, mut sub_rx: mpsc::Receiver<SubscriptionReq>,
    ) -> Result<()> {
        use zbus::Message;
        use zbus::message::Type;
        use zbus::MessageStream;

        let object_manager_match = zbus::MatchRule::builder()
            .msg_type(Type::Signal)
            .sender(SERVICE_NAME)?
            .interface("org.freedesktop.DBus.ObjectManager")?
            .build();
        
        let properties_match = zbus::MatchRule::builder()
            .msg_type(Type::Signal)
            .sender(SERVICE_NAME)?
            .interface("org.freedesktop.DBus.Properties")?
            .member("PropertiesChanged")?
            .build();

        let mut object_manager_stream = MessageStream::for_match_rule(
            object_manager_match,
            &connection,
            None,
        ).await?;
        let mut properties_stream = MessageStream::for_match_rule(
            properties_match,
            &connection,
            None,
        ).await?;

        tokio::spawn(async move {
            log::trace!("Starting event loop for {}", connection.unique_name().map(|n| n.as_str()).unwrap_or_default());

            struct Subscription {
                child_objects: bool,
                tx: mpsc::UnboundedSender<Event>,
            }
            let mut subs: HashMap<String, Vec<Subscription>> = HashMap::new();

            loop {
                select! {
                    msg_opt = object_manager_stream.next() => {
                        if let Some(msg) = msg_opt {
                            let msg = match msg {
                                Ok(msg) => msg,
                                Err(e) => {
                                    log::warn!("Error receiving ObjectManager signal: {}", e);
                                    continue;
                                }
                            };
                            
                            let header = msg.header();
                            let member = header.member();
                            if let Some(member) = member {
                                if member == "InterfacesAdded" {
                                    if let Ok((object, interfaces)) = msg.body().deserialize::<(zbus::zvariant::OwnedObjectPath, HashMap<String, HashMap<String, zbus::zvariant::OwnedValue>>)>() {
                                        // Check for parent path match for ObjectAdded event.
                                        let parent = crate::parent_path(&object);
                                        if let Some(parent_subs) = subs.get_mut(parent.as_str()) {
                                            let evt = Self::ObjectAdded {
                                                object: object.into(),
                                                interfaces: interfaces.into_keys().collect(),
                                            };
                                            log::trace!("Event: {:?}", &evt);
                                            parent_subs.retain(|sub| {
                                                if sub.child_objects {
                                                    sub.tx.unbounded_send(evt.clone()).is_ok()
                                                } else {
                                                    true
                                                }
                                            });
                                            if parent_subs.is_empty() {
                                                subs.remove(parent.as_str());
                                            }
                                        }
                                    }
                                } else if member == "InterfacesRemoved" {
                                    if let Ok((object, interfaces)) = msg.body().deserialize::<(zbus::zvariant::OwnedObjectPath, Vec<String>)>() {
                                        // Check for parent path match for ObjectRemoved event.
                                        let parent = crate::parent_path(&object);
                                        if let Some(parent_subs) = subs.get_mut(parent.as_str()) {
                                            let evt = Self::ObjectRemoved {
                                                object: object.into(),
                                                interfaces: interfaces.into_iter().collect(),
                                            };
                                            log::trace!("Event: {:?}", &evt);
                                            parent_subs.retain(|sub| {
                                                if sub.child_objects {
                                                    sub.tx.unbounded_send(evt.clone()).is_ok()
                                                } else {
                                                    true
                                                }
                                            });
                                            if parent_subs.is_empty() {
                                                subs.remove(parent.as_str());
                                            }
                                        }
                                    }
                                }
                            }
                        } else {
                            break;
                        }
                    }
                    msg_opt = properties_stream.next() => {
                        if let Some(msg) = msg_opt {
                            let msg = match msg {
                                Ok(msg) => msg,
                                Err(e) => {
                                    log::warn!("Error receiving Properties signal: {}", e);
                                    continue;
                                }
                            };

                            if let Ok((interface_name, changed_properties, _invalidated_properties)) = msg.body().deserialize::<(String, HashMap<String, zbus::zvariant::OwnedValue>, Vec<String>)>() {
                                if let Some(object) = msg.header().path() {
                                    let object_str = object.as_str();
                                    // Check for direct path match for PropertiesChanged event.
                                    if let Some(path_subs) = subs.get_mut(object_str) {
                                        let evt = Self::PropertiesChanged {
                                            object: object.clone().into(),
                                            interface: interface_name,
                                            changed: Arc::new(changed_properties),
                                        };
                                        log::trace!("Event: {:?}", &evt);
                                        path_subs.retain(|sub| sub.tx.unbounded_send(evt.clone()).is_ok());
                                        if path_subs.is_empty() {
                                            subs.remove(object_str);
                                        }
                                    }
                                }
                            }
                        } else {
                            break;
                        }
                    }
                    sub_req_opt = sub_rx.next() => {
                        match sub_req_opt {
                            Some(sub_req) => {
                                let sub = Subscription {
                                    child_objects: sub_req.child_objects,
                                    tx: sub_req.tx,
                                };
                                subs.entry(sub_req.path.to_string()).or_default().push(sub);
                                let _ = sub_req.ready_tx.send(());
                            }
                            None => break,
                        }
                    }
                }
            }
        });
        Ok(())
    }
}
