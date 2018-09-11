use bluetooth_session::BluetoothSession;
use dbus::{BusType, Connection, Message, MessageItem};
use std::error::Error;

static ADAPTER_INTERFACE: &'static str = "org.bluez.Adapter1";
static SERVICE_NAME: &'static str = "org.bluez";

pub struct BluetoothDiscoverySession<'a> {
    adapter: String,
    session: &'a BluetoothSession,
}

impl<'a> BluetoothDiscoverySession<'a> {
    pub fn create_session(
        session: &'a BluetoothSession,
        adapter: String,
    ) -> Result<BluetoothDiscoverySession, Box<Error>> {
        Ok(BluetoothDiscoverySession::new(session, adapter))
    }

    fn new(session: &'a BluetoothSession, adapter: String) -> BluetoothDiscoverySession<'a> {
        BluetoothDiscoverySession {
            adapter: adapter,
            session: session,
        }
    }

    fn call_method(&self, method: &str, param: Option<[MessageItem; 1]>) -> Result<(), Box<Error>> {
        let mut m = try!(Message::new_method_call(
            SERVICE_NAME,
            &self.adapter,
            ADAPTER_INTERFACE,
            method
        ));
        match param {
            Some(p) => m.append_items(&p),
            None => (),
        };
        try!(
            self.session
                .get_connection()
                .send_with_reply_and_block(m, 1000)
        );
        Ok(())
    }

    pub fn start_discovery(&self) -> Result<(), Box<Error>> {
        self.call_method("StartDiscovery", None)
    }

    pub fn stop_discovery(&self) -> Result<(), Box<Error>> {
        self.call_method("StopDiscovery", None)
    }
}
