use dbus::nonblock::SyncConnection;
use dbus_tokio::connection;
use futures::{channel::oneshot, lock::Mutex, Stream, StreamExt};
use std::{
    collections::HashMap,
    fmt::{Debug, Formatter},
    sync::Arc,
};
use tokio::task::spawn_blocking;

use crate::{adapter, all_dbus_objects, Adapter, ObjectEvent, Result};

/// Bluetooth session.
pub struct Session {
    connection: Arc<SyncConnection>,
    discovery_slots: Arc<Mutex<HashMap<String, oneshot::Receiver<()>>>>,
}

impl Debug for Session {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        write!(f, "Session {{ {} }}", self.connection.unique_name())
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
        Ok(Self {
            connection,
            discovery_slots: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Enumerate connected Bluetooth adapters and return their names.
    pub async fn adapter_names(&self) -> Result<Vec<String>> {
        let mut names = Vec::new();
        for (path, interfaces) in all_dbus_objects(&*self.connection).await? {
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
        Adapter::new(
            self.connection.clone(),
            adapter_name,
            self.discovery_slots.clone(),
        )
    }

    /// Stream adapter added and removed events.
    pub async fn adapter_events(&self) -> Result<impl Stream<Item = AdapterEvent>> {
        let obj_events = ObjectEvent::stream(self.connection.clone(), None).await?;
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
