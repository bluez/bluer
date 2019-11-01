use dbus::{arg::cast, arg::RefArg, arg::TypeMismatchError, arg::Variant, Message};
use std::collections::HashMap;

#[derive(Clone, Debug)]
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
    Value {
        object_path: String,
        value: Box<[u8]>,
    },
    RSSI {
        object_path: String,
        rssi: i16,
    },
    None,
}

impl BluetoothEvent {
    pub fn from(conn_msg: Message) -> Option<BluetoothEvent> {
        let result: Result<
            (&str, HashMap<String, Variant<Box<dyn RefArg>>>),
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

                if let Some(value) = properties.get("Value") {
                    if let Some(value) = cast::<Vec<u8>>(&value.0) {
                        let event = BluetoothEvent::Value {
                            object_path: object_path.clone(),
                            value: value.clone().into_boxed_slice(),
                        };

                        return Some(event);
                    }
                }

                if let Some(value) = properties.get("RSSI") {
                    if let Some(rssi) = cast::<i16>(&value.0) {
                        let event = BluetoothEvent::RSSI {
                            object_path: object_path.clone(),
                            rssi: *rssi,
                        };

                        return Some(event);
                    }
                }

                Some(BluetoothEvent::None)
            }
            Err(_err) => None,
        }
    }
}
