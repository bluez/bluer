use dbus::{
    nonblock::{
        stdintf::org_freedesktop_dbus::{
            ObjectManager, ObjectManagerInterfacesAdded, ObjectManagerInterfacesRemoved,
        },
        Proxy, SyncConnection,
    },
    strings::BusName,
    Path,
};
use dbus_tokio::connection;
use futures::{stream, Stream, StreamExt};
use lazy_static::lazy_static;
use std::{
    fmt::{Debug, Formatter},
    sync::Arc,
};
use tokio::{
    sync::mpsc::{self, UnboundedReceiver},
    task::spawn_blocking,
};
use tokio_stream::wrappers::UnboundedReceiverStream;

use crate::{adapter, Adapter, Result, SERVICE_NAME, TIMEOUT};

pub struct Session {
    connection: Arc<SyncConnection>,
}

impl Debug for Session {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        write!(f, "Session {{ {} }}", self.connection.unique_name())
    }
}

/// D-Bus object event.
pub(crate) enum ObjectEvent {
    /// Object or object interfaces added.
    Added(ObjectManagerInterfacesAdded),
    /// Object or object interfaces removed.
    Removed(ObjectManagerInterfacesRemoved),
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
        Ok(Self { connection })
    }

    /// D-Bus connection.
    pub(crate) fn connection(&self) -> &SyncConnection {
        &self.connection
    }

    fn parse_adapter_dbus_path<'a>(path: &'a Path) -> Option<&'a str> {
        path.strip_prefix(adapter::PREFIX)
    }

    /// Enumerate connected Bluetooth adapters and return their names.
    pub async fn adapter_names(&self) -> Result<Vec<String>> {
        let mut names = Vec::new();
        let p = Proxy::new(SERVICE_NAME, "/", TIMEOUT, self.connection());
        for (path, interfaces) in p.get_managed_objects().await? {
            match Self::parse_adapter_dbus_path(&path) {
                Some(name) if interfaces.contains_key(adapter::INTERFACE) => {
                    names.push(name.to_string());
                }
                _ => (),
            }
        }
        Ok(names)
    }

    /// Create an interface to the Bluetooth adapter with the specified name.
    pub fn adapter(&self, adapter_name: &str) -> Adapter {
        Adapter::new(self, adapter_name)
    }

    /// Stream adapter added and removed events.
    pub async fn adapter_events(&self) -> Result<impl Stream<Item = AdapterEvent>> {
        let obj_events = self.object_events(None).await?;
        let obj_events = UnboundedReceiverStream::new(obj_events);
        let events = obj_events.filter_map(|evt| async move {
            match evt {
                ObjectEvent::Added(added) if added.interfaces.contains_key(adapter::INTERFACE) => {
                    match Self::parse_adapter_dbus_path(&added.object) {
                        Some(name) => Some(AdapterEvent::Added(name.to_string())),
                        None => None,
                    }
                }
                ObjectEvent::Removed(removed) if removed.interfaces.iter().any(|i| i == adapter::INTERFACE) => {
                    match Self::parse_adapter_dbus_path(&removed.object) {
                        Some(name) => Some(AdapterEvent::Removed(name.to_string())),
                        None => None,
                    }
                }
                _ => None,
            }
        });
        Ok(events)
    }

    /// Stream D-Bus object events starting with specified path prefix.
    pub(crate) async fn object_events(
        &self, path_prefix: Option<Path<'static>>,
    ) -> Result<UnboundedReceiver<ObjectEvent>> {
        use dbus::message::SignalArgs;
        lazy_static! {
            static ref SERVICE_NAME_BUS: BusName<'static> = BusName::new(SERVICE_NAME).unwrap();
            static ref SERVICE_NAME_REF: Option<&'static BusName<'static>> = Some(&SERVICE_NAME_BUS);
        }
        let connection = self.connection.clone();

        let rule_add = ObjectManagerInterfacesAdded::match_rule(*SERVICE_NAME_REF, None);
        let msg_match_add = connection.add_match(rule_add).await?;
        let (msg_match_add, stream_add) = msg_match_add.msg_stream();

        let rule_removed = ObjectManagerInterfacesRemoved::match_rule(*SERVICE_NAME_REF, None);
        let msg_match_removed = connection.add_match(rule_removed).await?;
        let (msg_match_removed, stream_removed) = msg_match_removed.msg_stream();

        let mut stream = stream::select(stream_add, stream_removed);

        let has_prefix = move |path: &Path<'static>| match &path_prefix {
            Some(prefix) => path.starts_with(&prefix.to_string()),
            None => true,
        };

        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            while let Some(msg) = stream.next().await {
                if let Some(added) = ObjectManagerInterfacesAdded::from_message(&msg) {
                    if has_prefix(&added.object) {
                        if tx.send(ObjectEvent::Added(added)).is_err() {
                            break;
                        }
                    }
                } else if let Some(removed) = ObjectManagerInterfacesRemoved::from_message(&msg) {
                    if has_prefix(&removed.object) {
                        if tx.send(ObjectEvent::Removed(removed)).is_err() {
                            break;
                        }
                    }
                }
            }

            let _ = connection.remove_match(msg_match_add.token()).await;
            let _ = connection.remove_match(msg_match_removed.token()).await;
        });

        Ok(rx)
    }
}
