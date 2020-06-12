use bluetooth_utils;
use std::error::Error;
use dbus::{Connection, BusType, Message, MessageItem, Props};

static LEADVERTISING_MANAGER_INTERFACE: &'static str = "org.bluez.LEAdvertisingManager1";
static ADAPTER_INTERFACE: &'static str = "org.bluez.Adapter1";
static SERVICE_NAME: &'static str = "org.bluez";

#[derive(Debug)]
pub struct BluetoothAdvertisingManager {
    object_path: String,
    connection: Connection,
}

impl BluetoothAdvertisingManager {
    pub fn create_adv_manager() -> Result<BluetoothAdvertisingManager, Box<Error>> {
        let managers = try!(bluetooth_utils::get_ad_man());
        if managers.is_empty() {
            return Err(Box::from("Bluetooth adapter not found"))
        }

        let c = try!(Connection::get_private(BusType::System));
        println!("{:?}", c);
        Ok(BluetoothAdvertisingManager::new(managers[0].clone(), c))
    }

	pub fn new(object_path: String, connection: Connection)
           -> BluetoothAdvertisingManager {
        BluetoothAdvertisingManager {
            object_path: object_path,
            connection: connection,
        }
    }

    pub fn get_object_path(&self) -> String {
        self.object_path.clone()
    }

    fn call_method(&self, method: &str, param: Option<[MessageItem; 2]>) -> Result<(), Box<Error>> {
        let mut m = try!(Message::new_method_call(SERVICE_NAME, &self.object_path, LEADVERTISING_MANAGER_INTERFACE, method));
        match param {
            Some(p) => m.append_items(&p),
            None => (),
        };
        try!(self.connection.send_with_reply_and_block(m, 1000));
        Ok(())
    }

/*
 * Methods
 */

    pub fn register_advertisement(&self, param: [MessageItem; 2]) -> Result<(), Box<Error>> {
    	self.call_method("RegisterAdvertisement", Some(param))
    }

    pub fn get_conn(&self) -> &Connection {
        &self.connection
    }

    pub fn unregister_advertisement(&self, addata: &str) -> Result<(), Box<Error>> {
    	//self.call_method("UnregisterAdvertisement", Some([addata.into()]))
        Ok(())
    }
}