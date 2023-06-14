//! Bluetooth session.

use dbus::{
    arg::Variant,
    message::MatchRule,
    nonblock::{
        stdintf::org_freedesktop_dbus::{
            ObjectManagerInterfacesAdded, ObjectManagerInterfacesRemoved, PropertiesPropertiesChanged,
        },
        SyncConnection,
    },
    strings::BusName,
    Message,
};
use dbus_crossroads::{Crossroads, IfaceToken};
use dbus_tokio::connection;
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
    adapter,
    adv::Advertisement,
    agent::{Agent, AgentHandle, RegisteredAgent},
    all_dbus_objects, gatt,
    monitor::RegisteredMonitor,
    parent_path, Adapter, DiscoveryFilter, Error, ErrorKind, InternalErrorKind, Result, SERVICE_NAME,
};

#[cfg(feature = "mesh")]
use crate::mesh::{
    agent::RegisteredProvisionAgent, application::RegisteredApplication, element::RegisteredElement,
    network::Network, provisioner::RegisteredProvisioner,
};

#[cfg(feature = "rfcomm")]
use crate::rfcomm::{profile::RegisteredProfile, Profile, ProfileHandle};

/// Terminate TX and terminated RX for single session.
type SingleSessionTerm = (Weak<oneshot::Sender<()>>, oneshot::Receiver<()>);

/// Shared state of all objects in a Bluetooth session.
pub(crate) struct SessionInner {
    pub connection: Arc<SyncConnection>,
    pub crossroads: Mutex<Crossroads>,
    pub le_advertisment_token: IfaceToken<Advertisement>,
    pub gatt_reg_service_token: IfaceToken<Arc<gatt::local::RegisteredService>>,
    pub gatt_reg_characteristic_token: IfaceToken<Arc<gatt::local::RegisteredCharacteristic>>,
    pub gatt_reg_characteristic_descriptor_token: IfaceToken<Arc<gatt::local::RegisteredDescriptor>>,
    pub gatt_profile_token: IfaceToken<gatt::local::Profile>,
    pub agent_token: IfaceToken<Arc<RegisteredAgent>>,
    #[cfg(feature = "mesh")]
    pub application_token: IfaceToken<Arc<RegisteredApplication>>,
    #[cfg(feature = "mesh")]
    pub element_token: IfaceToken<Arc<RegisteredElement>>,
    #[cfg(feature = "mesh")]
    pub provisioner_token: IfaceToken<Arc<RegisteredApplication>>,
    #[cfg(feature = "mesh")]
    pub provision_agent_token: IfaceToken<Arc<RegisteredProvisionAgent>>,
    pub monitor_token: IfaceToken<Arc<RegisteredMonitor>>,
    #[cfg(feature = "rfcomm")]
    pub profile_token: IfaceToken<Arc<RegisteredProfile>>,
    pub single_sessions: Mutex<HashMap<dbus::Path<'static>, SingleSessionTerm>>,
    pub event_sub_tx: mpsc::Sender<SubscriptionReq>,
    dbus_task: JoinHandle<connection::IOResourceError>,
    pub adapter_discovery_filter: Mutex<HashMap<String, DiscoveryFilter>>,
}

impl SessionInner {
    pub async fn single_session(
        &self, path: &dbus::Path<'static>, start_fn: impl Future<Output = Result<()>>,
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

    pub async fn is_single_session_active(&self, path: &dbus::Path<'static>) -> bool {
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
        &self, path: dbus::Path<'static>, child_objects: bool,
    ) -> Result<mpsc::UnboundedReceiver<Event>> {
        Event::subscribe(&mut self.event_sub_tx.clone(), path, child_objects).await
    }
}

impl Drop for SessionInner {
    fn drop(&mut self) {
        // documentation for dbus_tokio::connection::IOResource indicates it is abortable
        self.dbus_task.abort();
    }
}

#[derive(Clone)]
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
        write!(f, "Session {{ {} }}", self.inner.connection.unique_name())
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
        let (resource, connection) = spawn_blocking(connection::new_system_sync).await??;
        let dbus_task = tokio::spawn(resource);
        log::trace!("Connected to D-Bus with unique name {}", &connection.unique_name());

        let mut crossroads = Crossroads::new();
        crossroads.set_async_support(Some((
            connection.clone(),
            Box::new(|x| {
                tokio::spawn(x);
            }),
        )));

        crossroads.set_object_manager_support(Some(connection.clone()));

        let le_advertisment_token = Advertisement::register_interface(&mut crossroads);
        let gatt_service_token = gatt::local::RegisteredService::register_interface(&mut crossroads);
        let gatt_reg_characteristic_token =
            gatt::local::RegisteredCharacteristic::register_interface(&mut crossroads);
        let gatt_reg_characteristic_descriptor_token =
            gatt::local::RegisteredDescriptor::register_interface(&mut crossroads);
        let gatt_profile_token = gatt::local::Profile::register_interface(&mut crossroads);
        let agent_token = RegisteredAgent::register_interface(&mut crossroads);
        let monitor_token = RegisteredMonitor::register_interface(&mut crossroads);
        #[cfg(feature = "rfcomm")]
        let profile_token = RegisteredProfile::register_interface(&mut crossroads);
        #[cfg(feature = "mesh")]
        let application_token = RegisteredApplication::register_interface(&mut crossroads);
        #[cfg(feature = "mesh")]
        let element_token = RegisteredElement::register_interface(&mut crossroads);
        #[cfg(feature = "mesh")]
        let provisioner_token = RegisteredProvisioner::register_interface(&mut crossroads);
        #[cfg(feature = "mesh")]
        let provision_agent_token = RegisteredProvisionAgent::register_interface(&mut crossroads);

        let (event_sub_tx, event_sub_rx) = mpsc::channel(1);
        Event::handle_connection(connection.clone(), event_sub_rx).await?;

        let inner = Arc::new(SessionInner {
            connection: connection.clone(),
            crossroads: Mutex::new(crossroads),
            le_advertisment_token,
            gatt_reg_service_token: gatt_service_token,
            gatt_reg_characteristic_token,
            gatt_reg_characteristic_descriptor_token,
            gatt_profile_token,
            agent_token,
            #[cfg(feature = "mesh")]
            application_token,
            #[cfg(feature = "mesh")]
            element_token,
            #[cfg(feature = "mesh")]
            provisioner_token,
            #[cfg(feature = "mesh")]
            provision_agent_token,
            monitor_token,
            #[cfg(feature = "rfcomm")]
            profile_token,
            single_sessions: Mutex::new(HashMap::new()),
            event_sub_tx,
            dbus_task,
            adapter_discovery_filter: Mutex::new(HashMap::new()),
        });

        let mc_callback = connection.add_match(MatchRule::new_method_call()).await?;
        let mc_inner = Arc::downgrade(&inner);
        tokio::spawn(async move {
            let (_mc_callback, mut mc_stream) = mc_callback.msg_stream();
            while let Some(msg) = mc_stream.next().await {
                let mc_inner = match mc_inner.upgrade() {
                    Some(inner) => inner,
                    None => return,
                };
                let mut crossroads = mc_inner.crossroads.lock().await;
                let _ = crossroads.handle_message(msg, &*mc_inner.connection);
            }
        });

        Ok(Self { inner })
    }

    /// Create an interface to the default Bluetooth adapter.
    ///
    /// If `hci0` is present it is used as the default adapter.
    /// Otherwise the adapter that is first by lexicographic sorting is used as default.
    ///
    /// If the system has no Bluetooth adapter an error with
    /// [ErrorKind::NotFound] is returned.
    pub async fn default_adapter(&self) -> Result<Adapter> {
        let mut names = self.adapter_names().await?;
        if names.iter().any(|name| name == adapter::DEFAULT_NAME) {
            self.adapter(adapter::DEFAULT_NAME)
        } else {
            names.sort();
            match names.first() {
                Some(name) => self.adapter(name),
                None => Err(Error::new(ErrorKind::NotFound)),
            }
        }
    }

    /// Enumerate connected Bluetooth adapters and return their names.
    pub async fn adapter_names(&self) -> Result<Vec<String>> {
        let mut names = Vec::new();
        for (path, interfaces) in all_dbus_objects(&self.inner.connection).await? {
            match Adapter::parse_dbus_path(&path) {
                Some(name) if interfaces.contains_key(adapter::INTERFACE) => {
                    names.push(name.to_string());
                }
                _ => (),
            }
        }
        Ok(names)
    }

    /// Create an interface to the Bluetooth adapter with the specified name.
    pub fn adapter(&self, adapter_name: &str) -> Result<Adapter> {
        Adapter::new(self.inner.clone(), adapter_name)
    }

    /// Create an interface for the Bluetooth mesh network
    #[cfg(feature = "mesh")]
    #[cfg_attr(docsrs, doc(cfg(feature = "mesh")))]
    pub async fn mesh(&self) -> Result<Network> {
        Network::new(self.inner.clone()).await
    }

    /// Registers a Bluetooth authorization agent handler.
    ///
    /// Every application can register its own agent to use
    /// that agent for all actions triggered by that application.
    ///
    /// It is not required by an application to register
    /// an agent. If an application chooses not to
    /// register an agent, the default agent is used. This
    /// is in most cases a good idea. Only applications
    /// like a pairing wizard should register their own
    /// agent.
    ///
    /// An application can only register one agent. Multiple
    /// agents per application are not supported.
    ///
    /// Drop the returned [AgentHandle] to unregister the agent.
    pub async fn register_agent(&self, agent: Agent) -> Result<AgentHandle> {
        let reg_agent = RegisteredAgent::new(agent);
        reg_agent.register(self.inner.clone()).await
    }

    /// This registers a [Bluetooth profile implementation](Profile) for RFCOMM connections.
    ///
    /// The returned [ProfileHandle] provides a stream of
    /// [connection requests](crate::rfcomm::ConnectRequest).
    ///
    /// Drop the handle to unregister the profile.
    #[cfg(feature = "rfcomm")]
    #[cfg_attr(docsrs, doc(cfg(feature = "rfcomm")))]
    pub async fn register_profile(&self, profile: Profile) -> Result<ProfileHandle> {
        let (req_tx, req_rx) = tokio::sync::mpsc::channel(1);
        let reg_profile = RegisteredProfile::new(req_tx);
        reg_profile.register(self.inner.clone(), profile, req_rx).await
    }

    /// Stream adapter added and removed events.
    pub async fn events(&self) -> Result<impl Stream<Item = SessionEvent>> {
        let obj_events = self.inner.events(adapter::PATH.into(), true).await?;
        let events = obj_events.filter_map(|evt| async move {
            match evt {
                Event::ObjectAdded { object, interfaces }
                    if interfaces.iter().any(|i| i == adapter::INTERFACE) =>
                {
                    Adapter::parse_dbus_path(&object).map(|name| SessionEvent::AdapterAdded(name.to_string()))
                }
                Event::ObjectRemoved { object, interfaces }
                    if interfaces.iter().any(|i| i == adapter::INTERFACE) =>
                {
                    Adapter::parse_dbus_path(&object).map(|name| SessionEvent::AdapterRemoved(name.to_string()))
                }
                _ => None,
            }
        });
        Ok(events)
    }
}

/// A D-Bus object or property event.
#[derive(Debug)]
pub(crate) enum Event {
    /// Object or object interfaces added.
    ObjectAdded { object: dbus::Path<'static>, interfaces: HashSet<String> },
    /// Object or object interfaces removed.
    ObjectRemoved { object: dbus::Path<'static>, interfaces: HashSet<String> },
    /// Properties changed.
    PropertiesChanged { object: dbus::Path<'static>, interface: String, changed: dbus::arg::PropMap },
}

impl Clone for Event {
    fn clone(&self) -> Self {
        match self {
            Self::ObjectAdded { object, interfaces } => {
                Self::ObjectAdded { object: object.clone(), interfaces: interfaces.clone() }
            }
            Self::ObjectRemoved { object, interfaces } => {
                Self::ObjectRemoved { object: object.clone(), interfaces: interfaces.clone() }
            }
            Self::PropertiesChanged { object, interface, changed } => Self::PropertiesChanged {
                object: object.clone(),
                interface: interface.clone(),
                changed: changed.iter().map(|(k, v)| (k.clone(), Variant(v.0.box_clone()))).collect(),
            },
        }
    }
}

/// D-Bus events subscription request.
pub(crate) struct SubscriptionReq {
    path: dbus::Path<'static>,
    child_objects: bool,
    tx: mpsc::UnboundedSender<Event>,
    ready_tx: oneshot::Sender<()>,
}

impl Event {
    /// Spawns a task that handles events for the specified connection.
    pub(crate) async fn handle_connection(
        connection: Arc<SyncConnection>, mut sub_rx: mpsc::Receiver<SubscriptionReq>,
    ) -> Result<()> {
        use dbus::message::SignalArgs;
        lazy_static! {
            static ref SERVICE_NAME_BUS: BusName<'static> = BusName::new(SERVICE_NAME).unwrap();
            static ref SERVICE_NAME_REF: Option<&'static BusName<'static>> = Some(&SERVICE_NAME_BUS);
        }

        let (msg_tx, mut msg_rx) = mpsc::unbounded();
        let handle_msg = move |msg: Message| {
            let _ = msg_tx.unbounded_send(msg);
            true
        };

        let rule_add = ObjectManagerInterfacesAdded::match_rule(*SERVICE_NAME_REF, None);
        let msg_match_add = connection.add_match(rule_add).await?.msg_cb(handle_msg.clone());

        let rule_removed = ObjectManagerInterfacesRemoved::match_rule(*SERVICE_NAME_REF, None);
        let msg_match_removed = connection.add_match(rule_removed).await?.msg_cb(handle_msg.clone());

        let rule_prop = PropertiesPropertiesChanged::match_rule(*SERVICE_NAME_REF, None);
        let msg_match_prop = connection.add_match(rule_prop).await?.msg_cb(handle_msg.clone());

        tokio::spawn(async move {
            log::trace!("Starting event loop for {}", &connection.unique_name());

            struct Subscription {
                child_objects: bool,
                tx: mpsc::UnboundedSender<Event>,
            }
            let mut subs: HashMap<String, Vec<Subscription>> = HashMap::new();

            loop {
                select! {
                    msg_opt = msg_rx.next() => {
                        match msg_opt {
                            Some(msg) => {
                                // Properties changed.
                                if let (Some(object), Some(PropertiesPropertiesChanged { interface_name, changed_properties, .. })) =
                                    (msg.path(), PropertiesPropertiesChanged::from_message(&msg))
                                {
                                    // Check for direct path match for PropertiesChanged event.
                                    if let Some(path_subs) = subs.get_mut(&*object) {
                                        let evt = Self::PropertiesChanged {
                                            object: object.clone().into_static(),
                                            interface: interface_name,
                                            changed: changed_properties,
                                        };
                                        log::trace!("Event: {:?}", &evt);
                                        path_subs.retain(|sub| sub.tx.unbounded_send(evt.clone()).is_ok());
                                        if path_subs.is_empty() {
                                            subs.remove(&*object);
                                        }
                                    }
                                }

                                // Objects added.
                                if let Some(ObjectManagerInterfacesAdded { object, interfaces }) =
                                    ObjectManagerInterfacesAdded::from_message(&msg)
                                {
                                    // Check for parent path match for ObjectAdded event.
                                    let parent = parent_path(&object);
                                    if let Some(parent_subs) = subs.get_mut(&*parent) {
                                        let evt = Self::ObjectAdded {
                                            object,
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
                                            subs.remove(&*parent);
                                        }
                                    }
                                }

                                // Object removed.
                                if let Some(ObjectManagerInterfacesRemoved { object, interfaces, .. }) =
                                    ObjectManagerInterfacesRemoved::from_message(&msg)
                                {
                                    // Remove subscriptions for removed object.
                                    // This ends the event streams of the subscriptions.
                                    if subs.remove(&*object).is_some() {
                                        log::trace!("Event subscription for {} ended because object was removed", &object);
                                    }

                                    // Check for parent path match for ObjectRemoved event.
                                    let parent = parent_path(&object);
                                    if let Some(parent_subs) = subs.get_mut(&*parent) {
                                        let evt = Self::ObjectRemoved { object, interfaces: interfaces.into_iter().collect() };
                                        log::trace!("Event: {:?}", &evt);
                                        parent_subs.retain(|sub| {
                                            if sub.child_objects {
                                                sub.tx.unbounded_send(evt.clone()).is_ok()
                                            } else {
                                                true
                                            }
                                        });
                                        if parent_subs.is_empty() {
                                            subs.remove(&*parent);
                                        }
                                    }
                                }
                            },
                            None => break,
                        }
                    },
                    sub_opt = sub_rx.next() => {
                        match sub_opt {
                            Some(SubscriptionReq { path, child_objects, tx, ready_tx }) => {
                                log::trace!("Adding event subscription for {} with child_objects={:?}", &path, &child_objects);
                                let _ = ready_tx.send(());
                                let path_subs = subs.entry(path.to_string()).or_default();
                                path_subs.push(Subscription {
                                    child_objects, tx
                                });
                            }
                            None => break,
                        }
                    }
                }
            }

            let _ = connection.remove_match(msg_match_add.token()).await;
            let _ = connection.remove_match(msg_match_removed.token()).await;
            let _ = connection.remove_match(msg_match_prop.token()).await;
            log::trace!("Terminated event loop for {}", &connection.unique_name());
        });

        Ok(())
    }

    /// Subscribe to D-Bus events for specified path.
    ///
    /// If `child_objects` is [true] events about *direct* child objects being added and removed
    /// will also be delivered.
    pub(crate) async fn subscribe(
        sub_tx: &mut mpsc::Sender<SubscriptionReq>, path: dbus::Path<'static>, child_objects: bool,
    ) -> Result<mpsc::UnboundedReceiver<Event>> {
        let (tx, rx) = mpsc::unbounded();
        let (ready_tx, ready_rx) = oneshot::channel();
        sub_tx
            .send(SubscriptionReq { path, child_objects, tx, ready_tx })
            .await
            .map_err(|_| Error::new(ErrorKind::Internal(InternalErrorKind::DBusConnectionLost)))?;
        ready_rx.await.map_err(|_| Error::new(ErrorKind::Internal(InternalErrorKind::DBusConnectionLost)))?;
        Ok(rx)
    }
}
