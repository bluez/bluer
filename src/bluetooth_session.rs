use dbus::{arg::cast, arg::RefArg, arg::TypeMismatchError, arg::Variant, BusType, Connection};
use std::collections::HashMap;
use std::error::Error;

use bluetooth_adapter::BluetoothAdapter;
use bluetooth_device::BluetoothDevice;

use std::sync::mpsc;

static BLUEZ_MATCH: &'static str = "sender='org.bluez'";

pub struct BluetoothSession {
    connection: Connection,
    powered_callback: Option<Box<Fn(BluetoothAdapter, &bool)>>,
    discovering_callback: Option<Box<Fn(BluetoothAdapter, &bool)>>,
    connected_callback: Option<Box<Fn(BluetoothDevice, &bool)>>,
    services_resolved_callback: Option<Box<Fn(BluetoothDevice, &bool)>>,
}

impl BluetoothSession {
    pub fn create_session() -> Result<BluetoothSession, Box<Error>> {
        let c = try!(Connection::get_private(BusType::System));
        Ok(BluetoothSession::new(c))
    }

    fn new(connection: Connection) -> BluetoothSession {
        BluetoothSession {
            connection: connection,
            powered_callback: None,
            discovering_callback: None,
            connected_callback: None,
            services_resolved_callback: None,
        }
    }

    pub fn get_connection(&self) -> &Connection {
        &self.connection
    }

    pub fn set_powered_callback(&mut self, callback: Box<Fn(BluetoothAdapter, &bool)>) {
        self.powered_callback = Some(callback);
    }

    pub fn set_discovering_callback(&mut self, callback: Box<Fn(BluetoothAdapter, &bool)>) {
        self.discovering_callback = Some(callback);
    }

    pub fn set_connected_callback(&mut self, callback: Box<Fn(BluetoothDevice, &bool)>) {
        self.connected_callback = Some(callback);
    }

    pub fn set_services_resolved_callback(&mut self, callback: Box<Fn(BluetoothDevice, &bool)>) {
        self.services_resolved_callback = Some(callback);
    }

    pub fn listen(&mut self, terminate: mpsc::Receiver<bool>) -> Result<(), Box<Error>> {
        self.connection.add_match(BLUEZ_MATCH)?;
        loop {
            for conn_msg in self.connection.incoming(1000) {
                let result: Result<
                    (&str, HashMap<String, Variant<Box<RefArg>>>),
                    TypeMismatchError,
                > = conn_msg.read2();

                match result {
                    Ok((_, properties)) => {
                        let object_path = conn_msg.path().unwrap().to_string();

                        if let Some(value) = properties.get("Powered") {
                            if let Some(powered) = cast::<bool>(&value.0) {
                                if let Some(ref callback) = self.powered_callback {
                                    callback(
                                        BluetoothAdapter::create_adapter(self, object_path.clone())
                                            .unwrap(),
                                        powered,
                                    );
                                }
                            }
                        }

                        if let Some(value) = properties.get("Discovering") {
                            if let Some(discovering) = cast::<bool>(&value.0) {
                                if let Some(ref callback) = self.discovering_callback {
                                    callback(
                                        BluetoothAdapter::create_adapter(self, object_path.clone())
                                            .unwrap(),
                                        discovering,
                                    );
                                }
                            }
                        }

                        if let Some(value) = properties.get("Connected") {
                            if let Some(c) = cast::<bool>(&value.0) {
                                if let Some(ref callback) = self.connected_callback {
                                    callback(BluetoothDevice::new(self, object_path.clone()), c);
                                }
                            }
                        }

                        if let Some(value) = properties.get("ServicesResolved") {
                            if let Some(s) = cast::<bool>(&value.0) {
                                println!("{} services_resolved={}", conn_msg.path().unwrap(), s);
                            }
                        }
                    }
                    _ => {}
                }
            }

            let t = terminate.try_recv();
            match t {
                Ok(true) => {
                    return Ok(());
                }
                _ => {}
            }
        }
    }
}
