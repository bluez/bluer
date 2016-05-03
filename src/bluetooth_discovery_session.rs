use dbus::{BusType, Connection, Message, MessageItem};
use std::error::Error;

static ADAPTER_INTERFACE: &'static str = "org.bluez.Adapter1";
static SERVICE_NAME: &'static str = "org.bluez";

#[derive(Debug)]
pub struct BluetoothDiscoverySession {
    adapter: String,
    connection: Connection,
}

impl BluetoothDiscoverySession {
    pub fn create_session(adapter: String) -> Result<BluetoothDiscoverySession, Box<Error>> {
        let c = try!(Connection::get_private(BusType::System));
        Ok(BluetoothDiscoverySession::new(adapter, c))
    }

    fn new(adapter: String, connection: Connection) -> BluetoothDiscoverySession {
        BluetoothDiscoverySession {
            adapter: adapter,
            connection: connection,
        }
    }

    fn call_method(&self, method: &str, param: Option<[MessageItem; 1]>) -> Result<(), Box<Error>> {
        let mut m = try!(Message::new_method_call(SERVICE_NAME, &self.adapter, ADAPTER_INTERFACE, method));
        match param {
            Some(p) => m.append_items(&p),
            None => (),
        };
        try!(self.connection.send_with_reply_and_block(m, 1000));
        Ok(())
    }

    pub fn start_discovery(&self) -> Result<(), Box<Error>> {
        self.call_method("StartDiscovery", None)
    }

    pub fn stop_discovery(&self) -> Result<(), Box<Error>> {
        self.call_method("StopDiscovery", None)
    }
}
