use crate::session::Session;
use crate::{bluetooth_utils, ok_or_str};
use dbus::arg::messageitem::MessageItem;

use std::error::Error;

static GATT_SERVICE_INTERFACE: &str = "org.bluez.GattService1";

#[derive(Clone, Debug)]
pub struct BluetoothGATTService<'a> {
    object_path: String,
    session: &'a Session,
}

impl<'a> BluetoothGATTService<'a> {
    pub fn new(session: &'a Session, object_path: &str) -> BluetoothGATTService<'a> {
        BluetoothGATTService {
            object_path: object_path.to_string(),
            session,
        }
    }

    pub fn get_id(&self) -> String {
        self.object_path.clone()
    }

    fn get_property(&self, prop: &str) -> Result<MessageItem, Box<dyn Error>> {
        bluetooth_utils::get_property(
            self.session.connection(),
            GATT_SERVICE_INTERFACE,
            &self.object_path,
            prop,
        )
    }

    /*
     * Properties
     */

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/gatt-api.txt#n33
    pub fn get_uuid(&self) -> Result<String, Box<dyn Error>> {
        let uuid = self.get_property("UUID")?;
        Ok(String::from(ok_or_str!(uuid.inner::<&str>())?))
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/gatt-api.txt#n37
    pub fn is_primary(&self) -> Result<bool, Box<dyn Error>> {
        let primary = self.get_property("Primary")?;
        ok_or_str!(primary.inner::<bool>())
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/gatt-api.txt#n42
    pub fn get_device(&self) -> Result<String, Box<dyn Error>> {
        let device = self.get_property("Device")?;
        Ok(String::from(ok_or_str!(device.inner::<&str>())?))
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/gatt-api.txt#n48
    pub fn get_includes(&self) -> Result<Vec<String>, Box<dyn Error>> {
        Err(Box::from("Not implemented"))
    }

    pub fn get_gatt_characteristics(&self) -> Result<Vec<String>, Box<dyn Error>> {
        bluetooth_utils::list_characteristics(self.session.connection(), &self.object_path)
    }
}
