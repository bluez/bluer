use crate::{Adapter, Result, SERVICE_NAME, TIMEOUT, adapter};
use std::fmt::Debug;
use std::{fmt::Formatter, sync::Arc};

use dbus::nonblock::{Proxy, SyncConnection, stdintf::org_freedesktop_dbus::ObjectManager};
use dbus_tokio::connection;
use tokio::task::spawn_blocking;

// static BLUEZ_MATCH: &str = "type='signal',sender='org.bluez'";


pub struct Session {
    connection: Arc<SyncConnection>,
}

impl Debug for Session {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        write!(f, "Session {{ {} }}", self.connection.unique_name())
    }
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

    // pub fn incoming(&self, timeout_ms: u32) -> ConnMsgs<&Connection> {
    //     self.connection.incoming(timeout_ms)
    // }

    /// Enumerate connected Bluetooth adapters and return their names.
    pub async fn adapter_names(&self) -> Result<Vec<String>> {
        let mut names = Vec::new();
        let p = Proxy::new(SERVICE_NAME, "/", TIMEOUT, self.connection());
        for (path, interfaces) in p.get_managed_objects().await? {
            if interfaces.contains_key(adapter::INTERFACE) {
                names.push(path.split('/').last().unwrap().to_string());
            }
        }
        Ok(names)
    }

    /// Create an interface to the Bluetooth adapter with the specified name.
    pub fn adapter(&self, adapter_name: &str) -> Adapter {
        Adapter::new(self, adapter_name)
    }

    pub(crate) async fn watch_prefix(&self, : )
}
