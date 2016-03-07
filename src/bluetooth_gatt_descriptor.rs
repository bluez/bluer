use dbus::{Connection, BusType, Message, MessageItem};
use bluetooth_utils;

static SERVICE_NAME: &'static str = "org.bluez";
static GATT_DESCRIPTOR_INTERFACE: &'static str = "org.bluez.GattDescriptor1";

#[derive(Clone, Debug)]
pub struct BluetoothGATTDescriptor {
    object_path: String,
}

impl BluetoothGATTDescriptor {
    pub fn new(object_path: String)
           -> BluetoothGATTDescriptor {
        BluetoothGATTDescriptor {
            object_path: object_path
        }
    }

    pub fn get_object_path(&self) -> String {
        self.object_path.clone()
    }

    fn get_property(&self, prop: &str) -> Result<MessageItem, String> {
        bluetooth_utils::get_property(GATT_DESCRIPTOR_INTERFACE, &self.object_path, prop)
    }

    pub fn get_uuid(&self) -> Result<String, String> {
        match self.get_property("UUID") {
            Ok(uuid) => Ok(String::from(uuid.inner::<&str>().unwrap())),
            Err(e) => Err(e),
        }
    }

    pub fn get_characteristic(&self) -> Result<String, String> {
        match self.get_property("Characteristic") {
            Ok(service) => Ok(String::from(service.inner::<&str>().unwrap())),
            Err(e) => Err(e),
        }
    }

    pub fn get_value(&self) -> Result<u8, String> {
        match self.get_property("Value") {
            Ok(value) => Ok(value.inner::<u8>().unwrap()),
            Err(e) => Err(e),
        }
    }

    pub fn read_value(&self) -> Result<Vec<u8>, String> {
        let c = match Connection::get_private(BusType::System) {
            Ok(conn) => conn,
            Err(_) => return Err(String::from("Error! Connecting to dbus."))
        };
        let m = Message::new_method_call(SERVICE_NAME, &self.object_path, GATT_DESCRIPTOR_INTERFACE, "ReadValue").unwrap();
        let reply = match c.send_with_reply_and_block(m, 1000) {
            Ok(r) => r,
            Err(_) => return Err(String::from("Error! Read value.")),
        };
        let items = reply.get_items();
        let z: &[MessageItem] = items.get(0).unwrap().inner().unwrap();
        let mut v: Vec<u8> = Vec::new();
        for i in z {
            v.push(i.inner::<u8>().unwrap());
        }
        Ok(v)
    }
}
