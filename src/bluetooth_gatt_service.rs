use bluetooth_session::BluetoothSession;
use bluetooth_utils;
use dbus::MessageItem;

use std::error::Error;

static GATT_SERVICE_INTERFACE: &'static str = "org.bluez.GattService1";

#[derive(Clone, Debug)]
pub struct BluetoothGATTService<'a> {
    object_path: String,
    session: &'a BluetoothSession,
}

impl<'a> BluetoothGATTService<'a> {
    pub fn new(session: &'a BluetoothSession, object_path: String) -> BluetoothGATTService {
        BluetoothGATTService {
            object_path: object_path,
            session: session,
        }
    }

    pub fn get_id(&self) -> String {
        self.object_path.clone()
    }

    fn get_property(&self, prop: &str) -> Result<MessageItem, Box<Error>> {
        bluetooth_utils::get_property(
            self.session.get_connection(),
            GATT_SERVICE_INTERFACE,
            &self.object_path,
            prop,
        )
    }

    /*
     * Properties
     */

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/gatt-api.txt#n33
    pub fn get_uuid(&self) -> Result<String, Box<Error>> {
        let uuid = try!(self.get_property("UUID"));
        Ok(String::from(uuid.inner::<&str>().unwrap()))
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/gatt-api.txt#n37
    pub fn is_primary(&self) -> Result<bool, Box<Error>> {
        let primary = try!(self.get_property("Primary"));
        Ok(primary.inner::<bool>().unwrap())
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/gatt-api.txt#n42
    pub fn get_device(&self) -> Result<String, Box<Error>> {
        let device = try!(self.get_property("Device"));
        Ok(String::from(device.inner::<&str>().unwrap()))
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/gatt-api.txt#n48
    pub fn get_includes(&self) -> Result<Vec<String>, Box<Error>> {
        Err(Box::from("Not implemented"))
    }

    pub fn get_gatt_characteristics(&self) -> Result<Vec<String>, Box<Error>> {
        bluetooth_utils::list_characteristics(self.session.get_connection(), &self.object_path)
    }
}
