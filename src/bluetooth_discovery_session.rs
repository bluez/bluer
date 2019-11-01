use crate::bluetooth_session::BluetoothSession;
use dbus::{Message, MessageItem, MessageItemArray, Signature};
use std::error::Error;

static ADAPTER_INTERFACE: &str = "org.bluez.Adapter1";
static SERVICE_NAME: &str = "org.bluez";

pub struct BluetoothDiscoverySession<'a> {
    adapter: String,
    session: &'a BluetoothSession,
}

impl<'a> BluetoothDiscoverySession<'a> {
    pub fn create_session(
        session: &'a BluetoothSession,
        adapter: String,
    ) -> Result<BluetoothDiscoverySession, Box<dyn Error>> {
        Ok(BluetoothDiscoverySession::new(session, adapter))
    }

    fn new(session: &'a BluetoothSession, adapter: String) -> BluetoothDiscoverySession<'a> {
        BluetoothDiscoverySession {
            adapter,
            session,
        }
    }

    fn call_method(&self, method: &str, param: Option<[MessageItem; 1]>) -> Result<(), Box<dyn Error>> {
        let mut m = Message::new_method_call(
            SERVICE_NAME,
            &self.adapter,
            ADAPTER_INTERFACE,
            method
        )?;
        if let Some(p) = param {
            m.append_items(&p);
        }
        self.session
            .get_connection()
            .send_with_reply_and_block(m, 1000)?;
        Ok(())
    }

    pub fn start_discovery(&self) -> Result<(), Box<dyn Error>> {
        self.call_method("StartDiscovery", None)
    }

    pub fn stop_discovery(&self) -> Result<(), Box<dyn Error>> {
        self.call_method("StopDiscovery", None)
    }

    pub fn set_discovery_filter(
        &self,
        uuids: Vec<String>,
        rssi: Option<i16>,
        pathloss: Option<u16>,
    ) -> Result<(), Box<dyn Error>> {
        let uuids = {
            let mut res: Vec<MessageItem> = Vec::new();
            for u in uuids {
                res.push(u.into());
            }
            res
        };

        let mut m = vec![MessageItem::DictEntry(
            Box::new("UUIDs".into()),
            Box::new(MessageItem::Variant(Box::new(
                MessageItem::new_array(uuids).unwrap(),
            ))),
        )];

        if let Some(rssi) = rssi {
            m.push(MessageItem::DictEntry(
                Box::new("RSSI".into()),
                Box::new(MessageItem::Variant(Box::new(rssi.into()))),
            ))
        }

        if let Some(pathloss) = pathloss {
            m.push(MessageItem::DictEntry(
                Box::new("Pathloss".into()),
                Box::new(MessageItem::Variant(Box::new(pathloss.into()))),
            ))
        }

        self.call_method(
            "SetDiscoveryFilter",
            Some([MessageItem::Array(
                MessageItemArray::new(m, Signature::from("a{sv}")).unwrap(),
            )]),
        )
    }
}

