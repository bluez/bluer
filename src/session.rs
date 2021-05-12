use dbus::{message::MatchRule, nonblock::SyncConnection};
use dbus_crossroads::{Crossroads, IfaceToken};
use dbus_tokio::connection;
use futures::{channel::oneshot, lock::Mutex, Stream, StreamExt};
use std::{
    collections::HashMap,
    fmt::{Debug, Formatter},
    sync::{Arc, Weak},
};
use tokio::task::spawn_blocking;

use crate::{adapter, all_dbus_objects, gatt, Adapter, LeAdvertisement, ObjectEvent, Result};

/// Shared state of all objects in a Bluetooth session.
pub(crate) struct SessionInner {
    pub connection: Arc<SyncConnection>,
    pub crossroads: Mutex<Crossroads>,
    pub le_advertisment_token: IfaceToken<LeAdvertisement>,
    pub gatt_service_token: IfaceToken<Arc<gatt::local::Service>>,
    pub gatt_characteristic_token: IfaceToken<Arc<gatt::local::Characteristic>>,
    pub gatt_characteristic_descriptor_token: IfaceToken<Arc<gatt::local::CharacteristicDescriptor>>,
    pub discovery_slots: Mutex<HashMap<String, oneshot::Receiver<()>>>,
    pub notify_slots: Mutex<HashMap<dbus::Path<'static>, (Weak<oneshot::Sender<()>>, oneshot::Receiver<()>)>>,
}

impl SessionInner {
    //pub fn notify_session(path: &dbus::Path, start_fn: impl FnOnce(oneshot::Receiver<()>, oneshot::Sender<()>))
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
pub enum AdapterEvent {
    /// Adapter added.
    Added(String),
    /// Adapter removed.
    Removed(String),
}

impl Session {
    /// Create a new Bluetooth session.
    pub async fn new() -> Result<Self> {
        let (resource, connection) = spawn_blocking(|| connection::new_system_sync()).await??;
        tokio::spawn(resource);
        //connection.request_name("io.crates.tokio-bluez", false, false, true).await?;

        let mut crossroads = Crossroads::new();
        crossroads.set_async_support(Some((
            connection.clone(),
            Box::new(|x| {
                tokio::spawn(x);
            }),
        )));

        let le_advertisment_token = LeAdvertisement::register_interface(&mut crossroads);
        let gatt_service_token = gatt::local::Service::register_interface(&mut crossroads);
        let gatt_characteristic_token = gatt::local::Characteristic::register_interface(&mut crossroads);
        let gatt_characteristic_descriptor_token =
            gatt::local::CharacteristicDescriptor::register_interface(&mut crossroads);

        let inner = Arc::new(SessionInner {
            connection: connection.clone(),
            crossroads: Mutex::new(crossroads),
            le_advertisment_token,
            gatt_service_token,
            gatt_characteristic_token,
            gatt_characteristic_descriptor_token,
            discovery_slots: Mutex::new(HashMap::new()),
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
    pub async fn adapter_events(&self) -> Result<impl Stream<Item = AdapterEvent>> {
        let obj_events = ObjectEvent::stream(self.inner.connection.clone(), None).await?;
        let events = obj_events.filter_map(|evt| async move {
            match evt {
                ObjectEvent::Added { object, interfaces }
                    if interfaces.iter().any(|i| i == adapter::INTERFACE) =>
                {
                    match Adapter::parse_dbus_path(&object) {
                        Some(name) => Some(AdapterEvent::Added(name.to_string())),
                        None => None,
                    }
                }
                ObjectEvent::Removed { object, interfaces }
                    if interfaces.iter().any(|i| i == adapter::INTERFACE) =>
                {
                    match Adapter::parse_dbus_path(&object) {
                        Some(name) => Some(AdapterEvent::Removed(name.to_string())),
                        None => None,
                    }
                }
                _ => None,
            }
        });
        Ok(events)
    }
}
