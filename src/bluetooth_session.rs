use dbus::{BusType, ConnMsgs, Connection};

use std::error::Error;

static BLUEZ_MATCH: &str = "type='signal',sender='org.bluez'";

#[derive(Debug)]
pub struct BluetoothSession {
    connection: Connection,
}

impl BluetoothSession {
    pub fn create_session(path: Option<&str>) -> Result<BluetoothSession, Box<dyn Error>> {
        let rule = {
            if let Some(path) = path {
                format!("{},path='{}'", BLUEZ_MATCH, path)
            } else {
                String::from(BLUEZ_MATCH)
            }
        };

        let c = Connection::get_private(BusType::System)?;
        c.add_match(rule.as_str())?;
        Ok(BluetoothSession::new(c))
    }

    fn new(connection: Connection) -> BluetoothSession {
        BluetoothSession {
            connection,
        }
    }

    pub fn get_connection(&self) -> &Connection {
        &self.connection
    }

    pub fn incoming(&self, timeout_ms: u32) -> ConnMsgs<&Connection> {
        self.connection.incoming(timeout_ms)
    }
}
