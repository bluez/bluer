use std::fmt::Debug;
use std::{error::Error, fmt::Formatter, sync::Arc};

use dbus::nonblock::SyncConnection;
use dbus_tokio::connection;

static BLUEZ_MATCH: &str = "type='signal',sender='org.bluez'";

//#[derive(Debug)]
pub struct BluetoothSession {
    connection: Arc<SyncConnection>,
}

impl Debug for BluetoothSession {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        write!(f, "BluetoothSession")
    }
}

impl BluetoothSession {
    pub async fn create_session(path: Option<&str>) -> Result<BluetoothSession, Box<dyn Error>> {
        let rule = {
            if let Some(path) = path {
                format!("{},path='{}'", BLUEZ_MATCH, path)
            } else {
                String::from(BLUEZ_MATCH)
            }
        };

        let (resource, connection) = connection::new_system_sync()?;
        tokio::spawn(resource);

        //c.add_match(rule.as_str()).await?;
        Ok(BluetoothSession { connection })
    }

    pub fn get_connection(&self) -> Arc<SyncConnection> {
        self.connection.clone()
    }

    // pub fn incoming(&self, timeout_ms: u32) -> ConnMsgs<&Connection> {
    //     self.connection.incoming(timeout_ms)
    // }
}
