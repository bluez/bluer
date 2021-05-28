//! Bluetooth session.

use dbus::{
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
use tokio::{select, task::spawn_blocking};

use crate::{adapter, all_dbus_objects, gatt, Adapter, Advertisement, Error, Result, SERVICE_NAME};

/// Shared state of all objects in a Bluetooth session.
pub(crate) struct SessionInner {
    pub connection: Arc<SyncConnection>,
    pub crossroads: Mutex<Crossroads>,
    pub le_advertisment_token: IfaceToken<Advertisement>,
    pub gatt_reg_service_token: IfaceToken<Arc<gatt::local::RegisteredService>>,
    pub gatt_reg_characteristic_token: IfaceToken<Arc<gatt::local::RegisteredCharacteristic>>,
    pub gatt_reg_characteristic_descriptor_token: IfaceToken<Arc<gatt::local::RegisteredDescriptor>>,
    pub gatt_profile_token: IfaceToken<gatt::local::Profile>,
    pub single_sessions: Mutex<HashMap<dbus::Path<'static>, (Weak<oneshot::Sender<()>>, oneshot::Receiver<()>)>>,
    pub event_sub_tx: mpsc::Sender<Subscription>,
}

impl SessionInner {
    pub async fn single_session(
        &self, path: &dbus::Path<'static>, start_fn: impl Future<Output = Result<()>>,
        stop_fn: impl Future<Output = ()> + Send + 'static,
    ) -> Result<SingleSessionToken> {
        let mut single_sessions = self.single_sessions.lock().await;

        if let Some((term_tx_weak, termed_rx)) = single_sessions.get_mut(&path) {
            match term_tx_weak.upgrade() {
                Some(term_tx) => return Ok(SingleSessionToken(term_tx)),
                None => {
                    let _ = termed_rx.await;
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
            stop_fn.await;
            let _ = termed_tx.send(());
            log::trace!("Terminated single session for {}", &path);
        });

        Ok(SingleSessionToken(term_tx))
    }

    pub async fn events(&self, path: dbus::Path<'static>) -> Result<mpsc::UnboundedReceiver<Event>> {
        Event::subscribe(&mut self.event_sub_tx.clone(), path).await
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
pub struct Session {
    inner: Arc<SessionInner>,
}

impl Debug for Session {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        write!(f, "Session {{ {} }}", self.inner.connection.unique_name())
    }
}

/// Bluetooth adapter event.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionEvent {
    /// Adapter added.
    AdapterAdded(String),
    /// Adapter removed.
    AdapterRemoved(String),
}

impl Session {
    /// Create a new Bluetooth session.
    pub async fn new() -> Result<Self> {
        let (resource, connection) = spawn_blocking(|| connection::new_system_sync()).await??;
        tokio::spawn(resource);
        log::trace!("Connected to D-Bus with unique name {}", &connection.unique_name());

        let mut crossroads = Crossroads::new();
        crossroads.set_async_support(Some((
            connection.clone(),
            Box::new(|x| {
                tokio::spawn(x);
            }),
        )));

        let le_advertisment_token = Advertisement::register_interface(&mut crossroads);
        let gatt_service_token = gatt::local::RegisteredService::register_interface(&mut crossroads);
        let gatt_reg_characteristic_token =
            gatt::local::RegisteredCharacteristic::register_interface(&mut crossroads);
        let gatt_characteristic_descriptor_token =
            gatt::local::RegisteredDescriptor::register_interface(&mut crossroads);
        let gatt_profile_token = gatt::local::Profile::register_interface(&mut crossroads);

        let (event_sub_tx, event_sub_rx) = mpsc::channel(1);
        Event::handle_connection(connection.clone(), event_sub_rx).await?;

        let inner = Arc::new(SessionInner {
            connection: connection.clone(),
            crossroads: Mutex::new(crossroads),
            le_advertisment_token,
            gatt_reg_service_token: gatt_service_token,
            gatt_reg_characteristic_token,
            gatt_reg_characteristic_descriptor_token: gatt_characteristic_descriptor_token,
            gatt_profile_token,
            single_sessions: Mutex::new(HashMap::new()),
            event_sub_tx,
        });

        let mc_callback = connection.add_match(MatchRule::new_method_call()).await?;
        let mc_inner = inner.clone();
        tokio::spawn(async move {
            let (_mc_callback, mut mc_stream) = mc_callback.msg_stream();
            while let Some(msg) = mc_stream.next().await {
                let mut crossroads = mc_inner.crossroads.lock().await;
                let _ = crossroads.handle_message(msg, &*mc_inner.connection);
            }
        });

        Ok(Self { inner })
    }

    /// Enumerate connected Bluetooth adapters and return their names.
    pub async fn adapter_names(&self) -> Result<Vec<String>> {
        let mut names = Vec::new();
        for (path, interfaces) in all_dbus_objects(&*self.inner.connection).await? {
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

    /// Stream adapter added and removed events.
    pub async fn events(&self) -> Result<impl Stream<Item = SessionEvent>> {
        let obj_events = self.inner.events("/".into()).await?;
        let events = obj_events.filter_map(|evt| async move {
            match evt {
                Event::ObjectAdded { object, interfaces }
                    if interfaces.iter().any(|i| i == adapter::INTERFACE) =>
                {
                    match Adapter::parse_dbus_path(&object) {
                        Some(name) => Some(SessionEvent::AdapterAdded(name.to_string())),
                        None => None,
                    }
                }
                Event::ObjectRemoved { object, interfaces }
                    if interfaces.iter().any(|i| i == adapter::INTERFACE) =>
                {
                    match Adapter::parse_dbus_path(&object) {
                        Some(name) => Some(SessionEvent::AdapterRemoved(name.to_string())),
                        None => None,
                    }
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

/// D-Bus events subscription.
pub(crate) struct Subscription {
    path: dbus::Path<'static>,
    tx: mpsc::UnboundedSender<Event>,
    ready_tx: Option<oneshot::Sender<()>>,
}

impl Event {
    /// Spawns a task that handles events for the specified connection.
    pub(crate) async fn handle_connection(
        connection: Arc<SyncConnection>, mut sub_rx: mpsc::Receiver<Subscription>,
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
            let mut subs: Vec<Subscription> = Vec::new();

            loop {
                select! {
                    msg_opt = msg_rx.next() => {
                        match msg_opt {
                            Some(msg) => {
                                let mut keep = Vec::new();
                                for sub in subs {
                                    let mut force_remove = false;
                                    let evt = {
                                        if let Some(ObjectManagerInterfacesAdded { object, interfaces }) = ObjectManagerInterfacesAdded::from_message(&msg) {
                                            if object.starts_with(&sub.path.to_string()) {
                                                Some(Self::ObjectAdded {
                                                    object,
                                                    interfaces: interfaces.into_iter().map(|(interface, _)| interface).collect(),
                                                })
                                            } else {
                                                None
                                            }
                                        } else if let Some(ObjectManagerInterfacesRemoved { object, interfaces, .. }) = ObjectManagerInterfacesRemoved::from_message(&msg) {
                                            if object.starts_with(&sub.path.to_string()) {
                                                force_remove = object == sub.path;
                                                Some(Self::ObjectRemoved { object, interfaces: interfaces.into_iter().collect() })
                                            } else {
                                                None
                                            }
                                        } else if let Some(PropertiesPropertiesChanged { interface_name, changed_properties, .. }) = PropertiesPropertiesChanged::from_message(&msg) {
                                            match msg.path() {
                                                Some(object) if object == sub.path =>
                                                    Some(Self::PropertiesChanged { object: sub.path.clone(), interface: interface_name, changed: changed_properties }),
                                                _ => None,
                                            }
                                        } else {
                                            None
                                        }
                                    };

                                    let sent_ok = match evt {
                                        Some(evt) => {
                                            log::trace!("Event: {:?}", &evt);
                                            sub.tx.unbounded_send(evt).is_ok()
                                        }
                                        None => true,
                                    };

                                    if sent_ok && !force_remove {
                                        keep.push(sub);
                                    } else {
                                        log::trace!("Removing event subscription for {}", &sub.path);
                                    }
                                }
                                subs = keep;
                            },
                            None => break,
                        }
                    },
                    sub_opt = sub_rx.next() => {
                        match sub_opt {
                            Some(mut sub) => {
                                log::trace!("Adding event subscription for {}", &sub.path);
                                if let Some(ready_tx) = sub.ready_tx.take() {
                                    let _ = ready_tx.send(());
                                }
                                subs.push(sub);
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
    pub(crate) async fn subscribe(
        sub_tx: &mut mpsc::Sender<Subscription>, path: dbus::Path<'static>,
    ) -> Result<mpsc::UnboundedReceiver<Event>> {
        let (tx, rx) = mpsc::unbounded();
        let (ready_tx, ready_rx) = oneshot::channel();
        sub_tx
            .send(Subscription { path, tx, ready_tx: Some(ready_tx) })
            .await
            .map_err(|_| Error::DBusConnectionLost)?;
        ready_rx.await.map_err(|_| Error::DBusConnectionLost)?;
        Ok(rx)
    }
}
