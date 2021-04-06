use crate::bluetooth_le_advertising_data::BluetoothAdvertisingData;
use crate::bluetooth_session::BluetoothSession;
use crate::bluetooth_utils;
use dbus::arg::messageitem::MessageItem;
use dbus::Message;
use hex::FromHex;
use std::collections::HashMap;
use std::error::Error;
use std::str::FromStr;
use crate::ok_or_str;

static DEVICE_INTERFACE: &str = "org.bluez.Device1";

/// Bluetooth device address type.
#[derive(Clone, Debug)]
pub enum BluetoothAddressType {
    /// Public address
    Public,
    /// Random address
    Random,
}

impl FromStr for BluetoothAddressType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "public" => Ok(Self::Public),
            "random" => Ok(Self::Random),
            _ => Err(format!("unknown address type: {}", &s)),
        }
    }
}

#[derive(Clone, Debug)]
pub struct BluetoothDevice<'a> {
    object_path: String,
    session: &'a BluetoothSession,
}

impl<'a> BluetoothDevice<'a> {
    pub fn new(session: &'a BluetoothSession, object_path: &str) -> BluetoothDevice<'a> {
        BluetoothDevice {
            object_path: object_path.to_string(),
            session,
        }
    }

    pub fn get_id(&self) -> String {
        self.object_path.clone()
    }

    pub fn get_addata(&self) -> Result<BluetoothAdvertisingData, Box<dyn Error>> {
        let addata =
            bluetooth_utils::list_addata_2(self.session.get_connection(), &self.object_path)?;

        if addata.is_empty() {
            return Err(Box::from("No addata found."));
        }
        Ok(BluetoothAdvertisingData::new(&self.session, &addata[0]))
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
    ) -> Result<Message, Box<dyn Error>> {
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
        Ok(String::from(ok_or_str!(address.inner::<&str>())?))
    }

    /// The Bluetooth device Address Type.
    pub fn get_address_type(&self) -> Result<BluetoothAddressType, Box<dyn Error>> {
        let address = self.get_property("AddressType")?;
        Ok(ok_or_str!(address.inner::<&str>())?.parse()?)
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n109
    pub fn get_name(&self) -> Result<String, Box<dyn Error>> {
        let name = self.get_property("Name")?;
        Ok(String::from(ok_or_str!(name.inner::<&str>())?))
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n121
    pub fn get_icon(&self) -> Result<String, Box<dyn Error>> {
        let icon = self.get_property("Icon")?;
        Ok(String::from(ok_or_str!(icon.inner::<&str>())?))
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n126
    pub fn get_class(&self) -> Result<u32, Box<dyn Error>> {
        let class = self.get_property("Class")?;
        ok_or_str!(class.inner::<u32>())
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n126
    pub fn get_appearance(&self) -> Result<u16, Box<dyn Error>> {
        let appearance = self.get_property("Appearance")?;
        ok_or_str!(appearance.inner::<u16>())
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n134
    pub fn get_uuids(&self) -> Result<Vec<String>, Box<dyn Error>> {
        let uuids = self.get_property("UUIDs")?;
        let z: &[MessageItem] = ok_or_str!(uuids.inner())?;
        let mut v: Vec<String> = Vec::new();
        for y in z {
            v.push(String::from(ok_or_str!(y.inner::<&str>())?));
        }
        Ok(v)
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n139
    pub fn is_paired(&self) -> Result<bool, Box<dyn Error>> {
        let paired = self.get_property("Paired")?;
        ok_or_str!(paired.inner::<bool>())
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n143
    pub fn is_connected(&self) -> Result<bool, Box<dyn Error>> {
        let connected = self.get_property("Connected")?;
        ok_or_str!(connected.inner::<bool>())
    }

    pub fn is_ready_to_receive(&self) -> Option<bool> {
        let is_connected: bool = self.is_connected().unwrap_or(false);
        let is_paired: bool = self.is_paired().unwrap_or(false);
        Some(is_paired && is_connected)
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n149
    pub fn set_trusted(&self, value: bool) -> Result<(), Box<dyn Error>> {
        self.set_property("Trusted", value, 1000)
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n149
    pub fn is_trusted(&self) -> Result<bool, Box<dyn Error>> {
        let trusted = self.get_property("Trusted")?;
        ok_or_str!(trusted.inner::<bool>())
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n185
    pub fn is_blocked(&self) -> Result<bool, Box<dyn Error>> {
        let blocked = self.get_property("Blocked")?;
        ok_or_str!(blocked.inner::<bool>())
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n161
    pub fn get_alias(&self) -> Result<String, Box<dyn Error>> {
        let alias = self.get_property("Alias")?;
        Ok(String::from(ok_or_str!(alias.inner::<&str>())?))
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n161
    pub fn set_alias(&self, value: &str) -> Result<(), Box<dyn Error>> {
        self.set_property("Alias", value, 1000)
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n174
    pub fn get_adapter(&self) -> Result<String, Box<dyn Error>> {
        let adapter = self.get_property("Adapter")?;
        Ok(String::from(ok_or_str!(adapter.inner::<&str>())?))
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n178
    pub fn is_legacy_pairing(&self) -> Result<bool, Box<dyn Error>> {
        let legacy_pairing = self.get_property("LegacyPairing")?;
        ok_or_str!(legacy_pairing.inner::<bool>())
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n189
    pub fn get_modalias(&self) -> Result<(String, u32, u32, u32), Box<dyn Error>> {
        let modalias = self.get_property("Modalias")?;
        let m = ok_or_str!(modalias.inner::<&str>())?;
        let ids: Vec<&str> = m.split(':').collect();

        let source = String::from(ids[0]);
        let vendor = Vec::from_hex(ids[1][1..5].to_string())?;
        let product = Vec::from_hex(ids[1][6..10].to_string())?;
        let device = Vec::from_hex(ids[1][11..15].to_string())?;

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

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n230
    pub fn get_rssi(&self) -> Result<i16, Box<dyn Error>> {
        let rssi = self.get_property("RSSI")?;
        ok_or_str!(rssi.inner::<i16>())
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n235
    pub fn get_tx_power(&self) -> Result<i16, Box<dyn Error>> {
        let tx_power = self.get_property("TxPower")?;
        ok_or_str!(tx_power.inner::<i16>())
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n240
    pub fn get_manufacturer_data(&self) -> Result<HashMap<u16, Vec<u8>>, Box<dyn Error>> {
        let manufacturer_data_array = self.get_property("ManufacturerData")?;
        let mut m = HashMap::new();
        let dict_vec = ok_or_str!(manufacturer_data_array
            .inner::<&[(MessageItem, MessageItem)]>())?;
        for (key, value) in dict_vec {
            let v = ok_or_str!(value
                .inner::<&MessageItem>()?
                .inner::<&Vec<MessageItem>>())?
                .iter()
                .map(|b| b.inner::<u8>().unwrap_or(0))
                .collect();
            m.insert(ok_or_str!(key.inner::<u16>())?, v);
        }
        Ok(m)
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n246
    pub fn get_service_data(&self) -> Result<HashMap<String, Vec<u8>>, Box<dyn Error>> {
        let service_data_array = self.get_property("ServiceData")?;
        let mut m = HashMap::new();
        let dict_vec = ok_or_str!(service_data_array
            .inner::<&[(MessageItem, MessageItem)]>())?;
        for (key, value) in dict_vec {
            let v = ok_or_str!(value
                .inner::<&MessageItem>()?
                .inner::<&Vec<MessageItem>>())?
                .iter()
                .map(|b| b.inner::<u8>().unwrap_or(0))
                .collect();
            m.insert(ok_or_str!(key.inner::<&str>())?.to_string(), v);
        }
        Ok(m)
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n251
    pub fn is_services_resolved(&self) -> Result<bool, Box<dyn Error>> {
        let services_resolved = self.get_property("ServicesResolved")?;
        ok_or_str!(services_resolved.inner::<bool>())
    }

    pub fn get_gatt_services(&self) -> Result<Vec<String>, Box<dyn Error>> {
        bluetooth_utils::list_services(self.session.get_connection(), &self.object_path)
    }

    /*
     * Methods
     */

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n12
    pub fn connect(&self, timeout_ms: i32) -> Result<Message, Box<dyn Error>> {
        self.call_method("Connect", None, timeout_ms)
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n43
    pub fn disconnect(&self) -> Result<Message, Box<dyn Error>> {
        self.call_method("Disconnect", None, 5000)
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n61
    pub fn connect_profile(&self, uuid: &str) -> Result<Message, Box<dyn Error>> {
        self.call_method("ConnectProfile", Some(&[uuid.into()]), 30000)
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n73
    pub fn disconnect_profile(&self, uuid: &str) -> Result<Message, Box<dyn Error>> {
        self.call_method("DisconnectProfile", Some(&[uuid.into()]), 5000)
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n88
    pub fn pair(&self, timeout_ms: i32) -> Result<Message, Box<dyn Error>> {
        self.call_method("Pair", None, timeout_ms)
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/device-api.txt#n115
    pub fn cancel_pairing(&self) -> Result<Message, Box<dyn Error>> {
        self.call_method("CancelPairing", None, 5000)
    }
}
