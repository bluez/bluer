use bluetooth_utils;
use dbus::MessageItem;
use std::collections::HashMap;
use std::error::Error;

static LEADVERTISING_DATA_INTERFACE: &'static str = "org.bluez.LEAdvertisement1";

#[derive(Clone, Debug)]
pub struct BluetoothAdvertisingData {
    object_path: String,
}

impl BluetoothAdvertisingData {
	pub fn new(object_path: String)
           -> BluetoothAdvertisingData {
        BluetoothAdvertisingData {
            object_path: object_path,
        }
    }

    pub fn get_object_path(&self) -> String {
        self.object_path.clone()
    }

    fn get_property(&self, prop: &str) -> Result<MessageItem, Box<Error>> {
        bluetooth_utils::get_property(LEADVERTISING_DATA_INTERFACE, &self.object_path, prop)
    }

/*
 * Properties
 */

    pub fn get_type(&self) -> Result<String, Box<Error>> {
        let type_ = try!(self.get_property("Type"));
        Ok(String::from(type_.inner::<&str>().unwrap()))
    }

    pub fn get_service_uuids(&self) -> Result<Vec<String>, Box<Error>> {
    	unimplemented!()
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n204
    pub fn get_manufacturer_data(&self) -> Result<HashMap<u16, Vec<u8>>, Box<Error>> {
        unimplemented!()
    }

    pub fn get_solicit_uuids(&self) -> Result<Vec<String>, Box<Error>> {
    	unimplemented!()
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n210
    pub fn get_service_data(&self) -> Result<HashMap<String, Vec<u8>>, Box<Error>> {
        unimplemented!()
    }

    pub fn include_tx_power(&self) -> Result<bool, Box<Error>> {
         let incl_tx_pow = try!(self.get_property("IncludeTxPower"));
         Ok(incl_tx_pow.inner::<bool>().unwrap())
    }
}