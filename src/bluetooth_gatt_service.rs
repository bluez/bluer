use dbus::{MessageItem};
use bluetooth_utils;

static GATT_SERVICE_INTERFACE: &'static str = "org.bluez.GattService1";

#[derive(Clone, Debug)]
pub struct BluetoothGATTService {
    object_path: String,
}

impl BluetoothGATTService {
    pub fn new(object_path: String)
           -> BluetoothGATTService {
        BluetoothGATTService {
            object_path: object_path
        }
    }

    pub fn get_object_path(&self) -> String {
        self.object_path.clone()
    }

    fn get_property(&self, prop: &str) -> Result<MessageItem, String> {
        bluetooth_utils::get_property(GATT_SERVICE_INTERFACE, &self.object_path, prop)
    }

    pub fn get_primary(&self) -> Result<bool, String> {
        match self.get_property("Primary") {
            Ok(primary) => Ok(primary.inner::<bool>().unwrap()),
            Err(e) => Err(e),
        }
    }

    pub fn get_characteristics(&self) -> Result<Vec<String>,String> {
        let characteristics = match self.get_property("Characteristics") {
            Ok(c) => c,
            Err(e) => return Err(e),
        };
        let z: &[MessageItem] = characteristics.inner().unwrap();
        let mut v: Vec<String> = Vec::new();
        for y in z {
            v.push(String::from(y.inner::<&str>().unwrap()));
        }
        Ok(v)
    }

    pub fn get_uuid(&self) -> Result<String, String> {
        match self.get_property("UUID") {
            Ok(uuid) => Ok(String::from(uuid.inner::<&str>().unwrap())),
            Err(e) => Err(e),
        }
    }
}
