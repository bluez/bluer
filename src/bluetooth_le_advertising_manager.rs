use bluetooth_utils;
use dbus::MessageItem;
use std::error::Error;

static LEADVERTISING_MANAGER_INTERFACE: &'static str = "org.bluez.LEAdvertisingManager1";

#[derive(Clone, Debug)]
pub struct BluetoothAdvertisingManager {
    object_path: String,
}

impl BluetoothAdvertisingManager {
	pub fn new(object_path: String)
           -> BluetoothAdvertisingManager {
        BluetoothAdvertisingManager {
            object_path: object_path
        }
    }

    pub fn get_object_path(&self) -> String {
        self.object_path.clone()
    }

    fn call_method(&self, method: &str, param: Option<[MessageItem; 1]>) -> Result<(), Box<Error>> {
        bluetooth_utils::call_method(LEADVERTISING_MANAGER_INTERFACE, &self.object_path, method, param)
    }

/*
 * Methods
 */

    pub fn register_advertisement(&self, addata: String) -> Result<(), Box<Error>> {
    	self.call_method("RegisterAdvertisement", Some([addata.into()]))
    }

    pub fn unregister_advertisement(&self, addata: String) -> Result<(), Box<Error>> {
    	self.call_method("UnregisterAdvertisement", Some([addata.into()]))
    }
}