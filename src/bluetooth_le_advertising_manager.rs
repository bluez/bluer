use bluetooth_utils;
use std::error::Error;
use dbus::{Connection, BusType, Message, MessageItem, Props};

static LEADVERTISING_MANAGER_INTERFACE: &'static str = "org.bluez.LEAdvertisingManager1";
static ADAPTER_INTERFACE: &'static str = "org.bluez.Adapter1";
static SERVICE_NAME: &'static str = "org.bluez";

#[derive(Clone, Debug)]
pub struct BluetoothAdvertisingManager {
    object_path: String,
}

impl BluetoothAdvertisingManager {
	pub fn new(object_path: String)
           -> BluetoothAdvertisingManager {
        BluetoothAdvertisingManager {
            object_path: object_path,
        }
    }

    pub fn init() -> Result<BluetoothAdvertisingManager, Box<Error>> {
        let managers = try!(bluetooth_utils::get_ad_man());

        if managers.is_empty() {
            return Err(Box::from("Bluetooth adapter not found"))
        }

        Ok(BluetoothAdvertisingManager::new(managers[0].clone()))
    }

    pub fn get_object_path(&self) -> String {
        self.object_path.clone()
    }

    fn call_method(&self, method: &str, param: Option<[MessageItem; 1]>) -> Result<(), Box<Error>> {
        bluetooth_utils::call_method(ADAPTER_INTERFACE, &self.object_path, method, param)
    }

/*
 * Methods
 */

    pub fn register_advertisement(&self, /*addata: Option<[MessageItem; 1]>, options: Option<[MessageItem; 1]>,*/ 
                                  param: Option<[MessageItem; 2]>) -> Result<(), Box<Error>> {
    	// self.call_method("RegisterAdvertisement", Some([addata.into()]))
        let c = try!(Connection::get_private(BusType::System));
        let mut m = try!(Message::new_method_call(SERVICE_NAME, &self.object_path, ADAPTER_INTERFACE, "RegisterAdvertisement"));
        match param {
            Some(p) => m.append_items(&p),
            None => (),
        };
        try!(c.send_with_reply_and_block(m, 1000));
        Ok(())
    }

    pub fn unregister_advertisement(&self, addata: &str) -> Result<(), Box<Error>> {
    	self.call_method("UnregisterAdvertisement", Some([addata.into()]))
    }
}