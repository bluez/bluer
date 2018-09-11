use dbus::{
    arg::cast, arg::RefArg, arg::TypeMismatchError, arg::Variant, BusType, ConnMsgs, Connection,
    Message,
};
use std::collections::HashMap;
use std::error::Error;

static BLUEZ_MATCH: &'static str = "type='signal',sender='org.bluez'";

pub struct BluetoothSession {
    connection: Connection,
}

#[derive(Clone)]
pub enum BluetoothEvent {
    Powered {
        object_path: String,
        powered: bool,
    },
    Discovering {
        object_path: String,
        discovering: bool,
    },
    Connected {
        object_path: String,
        connected: bool,
    },
    ServicesResolved {
        object_path: String,
        services_resolved: bool,
    },
    None,
}

impl BluetoothEvent {
    pub fn from(conn_msg: Message) -> Option<BluetoothEvent> {
        let result: Result<
            (&str, HashMap<String, Variant<Box<RefArg>>>),
            TypeMismatchError,
        > = conn_msg.read2();

        match result {
            Ok((_, properties)) => {
                let object_path = conn_msg.path().unwrap().to_string();

                if let Some(value) = properties.get("Powered") {
                    if let Some(powered) = cast::<bool>(&value.0) {
                        let event = BluetoothEvent::Powered {
                            object_path: object_path.clone(),
                            powered: *powered,
                        };

                        return Some(event);
                    }
                }

                if let Some(value) = properties.get("Discovering") {
                    if let Some(discovering) = cast::<bool>(&value.0) {
                        let event = BluetoothEvent::Discovering {
                            object_path: object_path.clone(),
                            discovering: *discovering,
                        };

                        return Some(event);
                    }
                }

                if let Some(value) = properties.get("Connected") {
                    if let Some(connected) = cast::<bool>(&value.0) {
                        let event = BluetoothEvent::Connected {
                            object_path: object_path.clone(),
                            connected: *connected,
                        };

                        return Some(event);
                    }
                }

                if let Some(value) = properties.get("ServicesResolved") {
                    if let Some(services_resolved) = cast::<bool>(&value.0) {
                        let event = BluetoothEvent::ServicesResolved {
                            object_path: object_path.clone(),
                            services_resolved: *services_resolved,
                        };

                        return Some(event);
                    }
                }

                Some(BluetoothEvent::None)
            }
            Err(err) => None,
        }
    }
}

impl BluetoothSession {
    pub fn create_session(path: Option<&str>) -> Result<BluetoothSession, Box<Error>> {
        let mut rule = {
            if let Some(path) = path {
                format!("{},path='{}'", BLUEZ_MATCH, path)
            } else {
                String::from(BLUEZ_MATCH)
            }
        };

        let c = try!(Connection::get_private(BusType::System));
        c.add_match(rule.as_str())?;
        Ok(BluetoothSession::new(c))
    }

    fn new(connection: Connection) -> BluetoothSession {
        BluetoothSession {
            connection: connection,
        }
    }

    pub fn get_connection(&self) -> &Connection {
        &self.connection
    }

    pub fn incoming(&self, timeout_ms: u32) -> ConnMsgs<&Connection> {
        self.connection.incoming(timeout_ms)
    }
}

unsafe impl Send for BluetoothEvent {}
