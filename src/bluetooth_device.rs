use crate::bluetooth_session::BluetoothSession;
use crate::bluetooth_utils;
use dbus::MessageItem;
use hex::FromHex;
use std::collections::HashMap;
use std::error::Error;

static DEVICE_INTERFACE: &str = "org.bluez.Device1";

#[derive(Clone, Debug)]
pub struct BluetoothDevice<'a> {
    object_path: String,
    session: &'a BluetoothSession,
}

impl<'a> BluetoothDevice<'a> {
    pub fn new(session: &'a BluetoothSession, object_path: String) -> BluetoothDevice {
        BluetoothDevice {
            object_path,
            session,
        }
    }

    pub fn get_id(&self) -> String {
        self.object_path.clone()
    }

    fn get_property(&self, prop: &str) -> Result<MessageItem, Box<dyn Error>> {
        bluetooth_utils::get_property(
            self.session.get_connection(),
            DEVICE_INTERFACE,
            &self.object_path,
            prop,
        )
    }

    fn set_property<T>(&self, prop: &str, value: T, timeout_ms: i32) -> Result<(), Box<dyn Error>>
    where
        T: Into<MessageItem>,
    {
        bluetooth_utils::set_property(
            self.session.get_connection(),
            DEVICE_INTERFACE,
            &self.object_path,
            prop,
            value,
            timeout_ms,
        )
    }

    fn call_method(
        &self,
        method: &str,
        param: Option<&[MessageItem]>,
        timeout_ms: i32,
    ) -> Result<(), Box<dyn Error>> {
        bluetooth_utils::call_method(
            self.session.get_connection(),
            DEVICE_INTERFACE,
            &self.object_path,
            method,
            param,
            timeout_ms,
        )
    }

    /*
     * Properties
     */
    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n105
    pub fn get_address(&self) -> Result<String, Box<dyn Error>> {
        let address = self.get_property("Address")?;
        Ok(String::from(address.inner::<&str>().unwrap()))
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n109
    pub fn get_name(&self) -> Result<String, Box<dyn Error>> {
        let name = self.get_property("Name")?;
        Ok(String::from(name.inner::<&str>().unwrap()))
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n121
    pub fn get_icon(&self) -> Result<String, Box<dyn Error>> {
        let icon = self.get_property("Icon")?;
        Ok(String::from(icon.inner::<&str>().unwrap()))
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n126
    pub fn get_class(&self) -> Result<u32, Box<dyn Error>> {
        let class = self.get_property("Class")?;
        Ok(class.inner::<u32>().unwrap())
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n126
    pub fn get_appearance(&self) -> Result<u16, Box<dyn Error>> {
        let appearance = self.get_property("Appearance")?;
        Ok(appearance.inner::<u16>().unwrap())
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n134
    pub fn get_uuids(&self) -> Result<Vec<String>, Box<dyn Error>> {
        let uuids = self.get_property("UUIDs")?;
        let z: &[MessageItem] = uuids.inner().unwrap();
        let mut v: Vec<String> = Vec::new();
        for y in z {
            v.push(String::from(y.inner::<&str>().unwrap()));
        }
        Ok(v)
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n139
    pub fn is_paired(&self) -> Result<bool, Box<dyn Error>> {
        let paired = self.get_property("Paired")?;
        Ok(paired.inner::<bool>().unwrap())
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n143
    pub fn is_connected(&self) -> Result<bool, Box<dyn Error>> {
        let connected = self.get_property("Connected")?;
        Ok(connected.inner::<bool>().unwrap())
    }

    pub fn is_ready_to_receive(&self) -> Option<bool> {
        let is_connected: bool = match self.is_connected() {
            Ok(value) => value,
            Err(_) => false,
        };
        let is_paired: bool = match self.is_paired() {
            Ok(value) => value,
            Err(_) => false,
        };
        Some(is_paired & is_connected)
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n149
    pub fn set_trusted(&self, value: bool) -> Result<(), Box<dyn Error>> {
        self.set_property("Trusted", value, 1000)
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n149
    pub fn is_trusted(&self) -> Result<bool, Box<dyn Error>> {
        let trusted = self.get_property("Trusted")?;
        Ok(trusted.inner::<bool>().unwrap())
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n154
    pub fn is_blocked(&self) -> Result<bool, Box<dyn Error>> {
        let blocked = self.get_property("Blocked")?;
        Ok(blocked.inner::<bool>().unwrap())
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n161
    pub fn get_alias(&self) -> Result<String, Box<dyn Error>> {
        let alias = self.get_property("Alias")?;
        Ok(String::from(alias.inner::<&str>().unwrap()))
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n161
    pub fn set_alias(&self, value: String) -> Result<(), Box<dyn Error>> {
        self.set_property("Alias", value, 1000)
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n174
    pub fn get_adapter(&self) -> Result<String, Box<dyn Error>> {
        let adapter = self.get_property("Adapter")?;
        Ok(String::from(adapter.inner::<&str>().unwrap()))
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n178
    pub fn is_legacy_pairing(&self) -> Result<bool, Box<dyn Error>> {
        let legacy_pairing = self.get_property("LegacyPairing")?;
        Ok(legacy_pairing.inner::<bool>().unwrap())
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n189
    pub fn get_modalias(&self) -> Result<(String, u32, u32, u32), Box<dyn Error>> {
        let modalias = self.get_property("Modalias")?;
        let m = modalias.inner::<&str>().unwrap();
        let ids: Vec<&str> = m.split(':').collect();

        let source = String::from(ids[0]);
        let vendor = Vec::from_hex(ids[1][1..5].to_string()).unwrap();
        let product = Vec::from_hex(ids[1][6..10].to_string()).unwrap();
        let device = Vec::from_hex(ids[1][11..15].to_string()).unwrap();

        Ok((
            source,
            (vendor[0] as u32) * 16 * 16 + (vendor[1] as u32),
            (product[0] as u32) * 16 * 16 + (product[1] as u32),
            (device[0] as u32) * 16 * 16 + (device[1] as u32),
        ))
    }

    pub fn get_vendor_id_source(&self) -> Result<String, Box<dyn Error>> {
        let (vendor_id_source, _, _, _) = self.get_modalias()?;
        Ok(vendor_id_source)
    }

    pub fn get_vendor_id(&self) -> Result<u32, Box<dyn Error>> {
        let (_, vendor_id, _, _) = self.get_modalias()?;
        Ok(vendor_id)
    }

    pub fn get_product_id(&self) -> Result<u32, Box<dyn Error>> {
        let (_, _, product_id, _) = self.get_modalias()?;
        Ok(product_id)
    }

    pub fn get_device_id(&self) -> Result<u32, Box<dyn Error>> {
        let (_, _, _, device_id) = self.get_modalias()?;
        Ok(device_id)
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n194
    pub fn get_rssi(&self) -> Result<i16, Box<dyn Error>> {
        let rssi = self.get_property("RSSI")?;
        Ok(rssi.inner::<i16>().unwrap())
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n199
    pub fn get_tx_power(&self) -> Result<i16, Box<dyn Error>> {
        let tx_power = self.get_property("TxPower")?;
        Ok(tx_power.inner::<i16>().unwrap())
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n204
    pub fn get_manufacturer_data(&self) -> Result<HashMap<u16, Vec<u8>>, Box<dyn Error>> {
        let manufacturer_data_array = self.get_property("ManufacturerData")?;
        let mut m = HashMap::new();
        let dict_vec = manufacturer_data_array
            .inner::<&Vec<MessageItem>>()
            .unwrap();
        for dict in dict_vec {
            let (key, value) = dict.inner::<(&MessageItem, &MessageItem)>().unwrap();
            let v = value
                .inner::<&MessageItem>()
                .unwrap()
                .inner::<&Vec<MessageItem>>()
                .unwrap()
                .iter()
                .map(|b| b.inner::<u8>().unwrap_or(0))
                .collect();
            m.insert(key.inner::<u16>().unwrap(), v);
        }
        Ok(m)
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n210
    pub fn get_service_data(&self) -> Result<HashMap<String, Vec<u8>>, Box<dyn Error>> {
        let service_data_array = self.get_property("ServiceData")?;
        let mut m = HashMap::new();
        let dict_vec = service_data_array.inner::<&Vec<MessageItem>>().unwrap();
        for dict in dict_vec {
            let (key, value) = dict.inner::<(&MessageItem, &MessageItem)>().unwrap();
            let v = value
                .inner::<&MessageItem>()
                .unwrap()
                .inner::<&Vec<MessageItem>>()
                .unwrap()
                .iter()
                .map(|b| b.inner::<u8>().unwrap_or(0))
                .collect();
            m.insert(key.inner::<&str>().unwrap().to_string(), v);
        }
        Ok(m)
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n215
    pub fn get_gatt_services(&self) -> Result<Vec<String>, Box<dyn Error>> {
        bluetooth_utils::list_services(self.session.get_connection(), &self.object_path)
    }

    /*
     * Methods
     */

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n12
    pub fn connect(&self, timeout_ms: i32) -> Result<(), Box<dyn Error>> {
        self.call_method("Connect", None, timeout_ms)
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n29
    pub fn disconnect(&self) -> Result<(), Box<dyn Error>> {
        self.call_method("Disconnect", None, 5000)
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n43
    pub fn connect_profile(&self, uuid: String) -> Result<(), Box<dyn Error>> {
        self.call_method("ConnectProfile", Some(&[uuid.into()]), 30000)
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n55
    pub fn disconnect_profile(&self, uuid: String) -> Result<(), Box<dyn Error>> {
        self.call_method("DisconnectProfile", Some(&[uuid.into()]), 5000)
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n70
    pub fn pair(&self) -> Result<(), Box<dyn Error>> {
        self.call_method("Pair", None, 60000)
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n97
    pub fn cancel_pairing(&self) -> Result<(), Box<dyn Error>> {
        self.call_method("CancelPairing", None, 5000)
    }
}
