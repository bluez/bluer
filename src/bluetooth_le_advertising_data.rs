use crate::bluetooth_session::BluetoothSession;
use crate::{bluetooth_utils, ok_or_str};
use dbus::arg::messageitem::MessageItem;
use std::collections::HashMap;
use std::error::Error;

static LEADVERTISING_DATA_INTERFACE: &str = "org.bluez.LEAdvertisement1";

#[derive(Clone, Debug)]
pub struct BluetoothAdvertisingData<'a> {
    object_path: String,
    session: &'a BluetoothSession,
}

impl<'a> BluetoothAdvertisingData<'a> {
    pub fn new(session: &'a BluetoothSession, object_path: &str) -> Self {
        BluetoothAdvertisingData {
            object_path: object_path.to_string(),
            session,
        }
    }

    pub fn get_object_path(&self) -> String {
        self.object_path.clone()
    }

    fn get_property(&self, prop: &str) -> Result<MessageItem, Box<dyn Error>> {
        bluetooth_utils::get_property(
            self.session.get_connection(),
            LEADVERTISING_DATA_INTERFACE,
            &self.object_path,
            prop,
        )
    }

    /*
     * Properties
     */

    pub fn get_type(&self) -> Result<String, Box<dyn Error>> {
        let type_ = self.get_property("Type")?;
        Ok(String::from(ok_or_str!(type_.inner::<&str>())?))
    }

    pub fn get_service_uuids(&self) -> Result<Vec<String>, Box<dyn Error>> {
        unimplemented!()
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n204
    pub fn get_manufacturer_data(&self) -> Result<HashMap<u16, Vec<u8>>, Box<dyn Error>> {
        unimplemented!()
    }

    pub fn get_solicit_uuids(&self) -> Result<Vec<String>, Box<dyn Error>> {
        unimplemented!()
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n210
    pub fn get_service_data(&self) -> Result<HashMap<String, Vec<u8>>, Box<dyn Error>> {
        unimplemented!()
    }

    pub fn include_tx_power(&self) -> Result<bool, Box<dyn Error>> {
        let incl_tx_pow = self.get_property("IncludeTxPower")?;
        ok_or_str!(incl_tx_pow.inner::<bool>())
    }
}
