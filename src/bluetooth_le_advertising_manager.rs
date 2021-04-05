use crate::bluetooth_utils;
use dbus::arg::messageitem::MessageItem;
use dbus::ffidisp::{BusType, Connection};
use dbus::Message;
use std::error::Error;

static LEADVERTISING_MANAGER_INTERFACE: &str = "org.bluez.LEAdvertisingManager1";

#[derive(Debug)]
pub struct BluetoothAdvertisingManager {
    object_path: String,
    connection: Connection,
}

impl BluetoothAdvertisingManager {
    pub fn create_adv_manager() -> Result<BluetoothAdvertisingManager, Box<dyn Error>> {
        let managers = bluetooth_utils::get_ad_man()?;
        if managers.is_empty() {
            return Err(Box::from("Bluetooth adapter not found"));
        }

        let c = Connection::get_private(BusType::System)?;
        //println!("{:?}", c);
        Ok(BluetoothAdvertisingManager::new(&managers[0], c))
    }

    pub fn new(object_path: &str, connection: Connection) -> BluetoothAdvertisingManager {
        BluetoothAdvertisingManager {
            object_path: object_path.to_string(),
            connection,
        }
    }

    pub fn get_object_path(&self) -> String {
        self.object_path.clone()
    }

    fn call_method(
        &self,
        method: &str,
        param: Option<&[MessageItem]>,
    ) -> Result<(), Box<dyn Error>> {
        let mut m = Message::new_method_call(
            bluetooth_utils::SERVICE_NAME,
            &self.object_path,
            LEADVERTISING_MANAGER_INTERFACE,
            method,
        )?;
        if let Some(p) = param {
            m.append_items(&p);
        }
        self.connection.send_with_reply_and_block(m, 1000)?;
        Ok(())
    }

    /*
     * Methods
     */

    pub fn register_advertisement(&self, param: [MessageItem; 2]) -> Result<(), Box<dyn Error>> {
        self.call_method("RegisterAdvertisement", Some(&param))
    }

    pub fn get_conn(&self) -> &Connection {
        &self.connection
    }

    pub fn unregister_advertisement(&self, addata: &str) -> Result<(), Box<dyn Error>> {
        self.call_method("UnregisterAdvertisement", Some(&[addata.into()]))
    }
}
