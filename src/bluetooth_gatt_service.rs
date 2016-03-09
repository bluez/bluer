use dbus::{MessageItem};
use bluetooth_utils;

use std::error::Error;

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

    fn get_property(&self, prop: &str) -> Result<MessageItem, Box<Error>> {
        bluetooth_utils::get_property(GATT_SERVICE_INTERFACE, &self.object_path, prop)
    }

/*
 * Properties
 */

    pub fn get_primary(&self) -> Result<bool, Box<Error>> {
        let primary = try!(self.get_property("Primary"));
        Ok(primary.inner::<bool>().unwrap())
    }

    pub fn get_characteristics(&self) -> Result<Vec<String>,Box<Error>> {
        let characteristics = try!(self.get_property("Characteristics"));
        let z: &[MessageItem] = characteristics.inner().unwrap();
        let mut v: Vec<String> = Vec::new();
        for y in z {
            v.push(String::from(y.inner::<&str>().unwrap()));
        }
        Ok(v)
    }

    pub fn get_uuid(&self) -> Result<String, Box<Error>> {
        let uuid = try!(self.get_property("UUID"));
        Ok(String::from(uuid.inner::<&str>().unwrap()))
    }

    pub fn get_device(&self) -> Result<String, Box<Error>> {
        let device = try!(self.get_property("Device"));
        Ok(String::from(device.inner::<&str>().unwrap()))
    }
}
