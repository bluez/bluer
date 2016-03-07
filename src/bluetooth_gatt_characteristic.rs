use dbus::{Connection, BusType, Message, MessageItem};
use bluetooth_utils;

static SERVICE_NAME: &'static str = "org.bluez";
static GATT_CHARACTERISTIC_INTERFACE: &'static str = "org.bluez.GattCharacteristic1";

#[derive(Clone, Debug)]
pub struct BluetoothGATTCharacteristic {
    object_path: String,
}

impl BluetoothGATTCharacteristic {
    pub fn new(object_path: String)
           -> BluetoothGATTCharacteristic {
        BluetoothGATTCharacteristic {
            object_path: object_path
        }
    }

    pub fn get_object_path(&self) -> String {
        self.object_path.clone()
    }

    fn get_property(&self, prop: &str) -> Result<MessageItem, String> {
        bluetooth_utils::get_property(GATT_CHARACTERISTIC_INTERFACE, &self.object_path, prop)
    }

    pub fn get_uuid(&self) -> Result<String, String> {
        match self.get_property("UUID") {
            Ok(uuid) => Ok(String::from(uuid.inner::<&str>().unwrap())),
            Err(e) => Err(e),
        }
    }

    pub fn get_service(&self) -> Result<String, String> {
        match self.get_property("Service") {
            Ok(service) => Ok(String::from(service.inner::<&str>().unwrap())),
            Err(e) => Err(e),
        }
    }

    pub fn get_value(&self) -> Result<Vec<u8>, String> {
        let value = match self.get_property("Value") {
            Ok(v) => v,
            Err(e) => return Err(e),
        };
        let z: &[MessageItem] = value.inner().unwrap();
        let mut v: Vec<u8> = Vec::new();
        for y in z {
            v.push(y.inner::<u8>().unwrap());
        }
        Ok(v)
    }

    pub fn get_descriptors(&self) -> Result<Vec<String>,String> {
        let descriptors = match self.get_property("Descriptors") {
            Ok(d) => d,
            Err(e) => return Err(e),
        };
        let z: &[MessageItem] = descriptors.inner().unwrap();
        let mut v: Vec<String> = Vec::new();
        for y in z {
            v.push(String::from(y.inner::<&str>().unwrap()));
        }
        Ok(v)
    }

    pub fn get_flags(&self) -> Result<Vec<String>,String> {
        let flags = match self.get_property("Flags") {
            Ok(f) => f,
            Err(e) => return Err(e),
        };
        let z: &[MessageItem] = flags.inner().unwrap();
        let mut v: Vec<String> = Vec::new();
        for y in z {
            v.push(String::from(y.inner::<&str>().unwrap()));
        }
        Ok(v)
    }

    pub fn is_notifying(&self) -> Result<bool, String> {
        match self.get_property("Notifying") {
            Ok(notifying) => Ok(notifying.inner::<bool>().unwrap()),
            Err(e) => Err(e),
        }
    }

    pub fn read_value(&self) -> Result<Vec<u8>, String> {
        let c = match Connection::get_private(BusType::System) {
            Ok(conn) => conn,
            Err(_) => return Err(String::from("Error! Connecting to dbus."))
        };
        let m = Message::new_method_call(SERVICE_NAME, &self.object_path, GATT_CHARACTERISTIC_INTERFACE, "ReadValue").unwrap();
        let reply = match c.send_with_reply_and_block(m, 1000) {
            Ok(r) => r,
            Err(_) => return Err(String::from("Error! Read value.")),
        };
        let items: MessageItem = match reply.get1() {
            Some(i) => i,
            None => return Err(String::from("Error! Read value.")),
        };
        let z: &[MessageItem] = items.inner().unwrap();
        let mut v: Vec<u8> = Vec::new();
        for i in z {
            v.push(i.inner::<u8>().unwrap());
        }
        Ok(v)
    }
}
