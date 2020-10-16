use crate::bluetooth_session::BluetoothSession;
use crate::bluetooth_utils;
use crate::bluetooth_event::BluetoothEvent;
use dbus::{Message, Signature};
use dbus::ffidisp::{Connection, BusType};
use dbus::arg::messageitem::{MessageItem, MessageItemDict};

use std::error::Error;
use dbus::arg::Variant;

static SERVICE_NAME: &str = "org.bluez";
static GATT_DESCRIPTOR_INTERFACE: &str = "org.bluez.GattDescriptor1";

#[derive(Clone, Debug)]
pub struct BluetoothGATTDescriptor<'a> {
    object_path: String,
    session: &'a BluetoothSession,
}

impl<'a> BluetoothGATTDescriptor<'a> {
    pub fn new(session: &'a BluetoothSession, object_path: String) -> BluetoothGATTDescriptor {
        BluetoothGATTDescriptor {
            object_path,
            session,
        }
    }

    pub fn get_id(&self) -> String {
        self.object_path.clone()
    }

    fn get_property(&self, prop: &str) -> Result<MessageItem, Box<dyn Error>> {
        bluetooth_utils::get_property(
            self.session.get_connection(),
            GATT_DESCRIPTOR_INTERFACE,
            &self.object_path,
            prop,
        )
    }

    fn call_method(
        &self,
        method: &str,
        param: Option<&[MessageItem]>,
        timeout_ms: i32,
    ) -> Result<Message, Box<dyn Error>> {
        bluetooth_utils::call_method(
            self.session.get_connection(),
            GATT_DESCRIPTOR_INTERFACE,
            &self.object_path,
            method,
            param,
            timeout_ms,
        )
    }

    /*
     * Properties
     */

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/gatt-api.txt#n198
    pub fn get_uuid(&self) -> Result<String, Box<dyn Error>> {
        let uuid = self.get_property("UUID")?;
        Ok(String::from(uuid.inner::<&str>().unwrap()))
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/gatt-api.txt#n202
    pub fn get_characteristic(&self) -> Result<String, Box<dyn Error>> {
        let service = self.get_property("Characteristic")?;
        Ok(String::from(service.inner::<&str>().unwrap()))
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/gatt-api.txt#n207
    pub fn get_value(&self) -> Result<Vec<u8>, Box<dyn Error>> {
        let value = self.get_property("Value")?;
        let z: &[MessageItem] = value.inner().unwrap();
        let mut v: Vec<u8> = Vec::new();
        for y in z {
            v.push(y.inner::<u8>().unwrap());
        }
        Ok(v)
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/gatt-api.txt#n213
    pub fn get_flags(&self) -> Result<Vec<String>, Box<dyn Error>> {
        let flags = self.get_property("Flags")?;
        let z: &[MessageItem] = flags.inner().unwrap();
        let mut v: Vec<String> = Vec::new();
        for y in z {
            v.push(String::from(y.inner::<&str>().unwrap()));
        }
        Ok(v)
    }

    /*
     * Methods
     */

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/gatt-api.txt#n174
    pub fn read_value(&self, offset: Option<u16>) -> Result<Vec<u8>, Box<dyn Error>> {
        let c = Connection::get_private(BusType::System)?;
        let mut m = Message::new_method_call(
            SERVICE_NAME,
            &self.object_path,
            GATT_DESCRIPTOR_INTERFACE,
            "ReadValue"
        )?;
        m.append_items(&[MessageItem::Dict(
            MessageItemDict::new(
                match offset {
                    Some(o) => vec![(
                        "offset".into(),
                        MessageItem::Variant(Box::new(o.into())),
                    )],
                    None => vec![],
                },
                Signature::make::<String>(),
                Signature::make::<Variant<u8>>(),
            ).unwrap(),
        )]);
        let reply = c.send_with_reply_and_block(m, 1000)?;
        let items: MessageItem = reply.get1().unwrap();
        let z: &[MessageItem] = items.inner().unwrap();
        let mut v: Vec<u8> = Vec::new();
        for i in z {
            v.push(i.inner::<u8>().unwrap());
        }
        Ok(v)
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/gatt-api.txt#n186
    pub fn write_value<I: Into<&'a[u8]>>(&self, values: I, offset: Option<u16>) -> Result<Option<BluetoothEvent>, Box<dyn Error>> {
        let args = values
            .into()
            .iter()
            .map(|v| MessageItem::from(*v))
            .collect();

        let message = self.call_method(
            "WriteValue",
            Some(&[
                MessageItem::new_array(args).unwrap(),
                MessageItem::Dict(
                    MessageItemDict::new(
                        match offset {
                            Some(o) => vec![(
                                "offset".into(),
                                MessageItem::Variant(Box::new(o.into())),
                            )],
                            None => vec![],
                        },
                        Signature::make::<String>(),
                        Signature::make::<Variant<u8>>(),
                    ).unwrap(),
                ),
            ]),
            1000,
        )?;

        Ok(BluetoothEvent::from(message))
    }
}
