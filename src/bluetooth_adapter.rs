use crate::bluetooth_device::BluetoothDevice;
use crate::bluetooth_le_advertising_data::BluetoothAdvertisingData;
use crate::bluetooth_session::BluetoothSession;
use crate::bluetooth_utils;
use dbus::{Path, arg::{Append, AppendAll, Arg, Get, ReadAll}};
use hex::FromHex;
use std::error::Error;
use std::collections::HashMap;

static ADAPTER_INTERFACE: &str = "org.bluez.Adapter1";

#[derive(Clone, Debug)]
pub struct BluetoothAdapter<'a> {
    object_path: String,
    session: &'a BluetoothSession,
}

impl<'a> BluetoothAdapter<'a> {
    fn new(session: &'a BluetoothSession, object_path: &str) -> BluetoothAdapter<'a> {
        BluetoothAdapter {
            object_path: object_path.to_string(),
            session,
        }
    }

    pub async fn init(session: &BluetoothSession) -> Result<BluetoothAdapter<'_>, Box<dyn Error>> {
        let adapters = bluetooth_utils::get_adapters(&session.get_connection()).await?;

        if adapters.is_empty() {
            return Err(Box::from("Bluetooth adapter not found"));
        }

        Ok(BluetoothAdapter::new(session, &adapters[0]))
    }

    pub async fn create_adapter(
        session: &'a BluetoothSession,
        object_path: &str,
    ) -> Result<BluetoothAdapter<'a>, Box<dyn Error>> {
        let adapters = bluetooth_utils::get_adapters(&session.get_connection()).await?;

        for adapter in adapters {
            if adapter == object_path {
                return Ok(BluetoothAdapter::new(session, &adapter));
            }
        }
        Err(Box::from("Bluetooth adapter not found"))
    }

    pub fn get_id(&self) -> String {
        self.object_path.clone()
    }

    pub async fn get_first_device(&self) -> Result<BluetoothDevice<'_>, Box<dyn Error>> {
        let devices =
            bluetooth_utils::list_devices(&self.session.get_connection(), &self.object_path).await?;

        if devices.is_empty() {
            return Err(Box::from("No device found."));
        }
        Ok(BluetoothDevice::new(self.session, &devices[0]))
    }

    pub async fn get_addata(&self) -> Result<BluetoothAdvertisingData<'_>, Box<dyn Error>> {
        let addata =
            bluetooth_utils::list_addata_1(&self.session.get_connection(), &self.object_path).await?;

        if addata.is_empty() {
            return Err(Box::from("No addata found."));
        }
        Ok(BluetoothAdvertisingData::new(&self.session, &addata[0]))
    }

    pub async fn get_device_list(&self) -> Result<Vec<String>, Box<dyn Error>> {
        bluetooth_utils::list_devices(&self.session.get_connection(), &self.object_path).await
    }

    async fn get_property<R>(&self, prop: &str) -> Result<R, Box<dyn Error>> 
    where R: for<'b> Get<'b> + 'static

    {
        bluetooth_utils::get_property(
            &self.session.get_connection(),
            ADAPTER_INTERFACE,
            &self.object_path,
            prop,
        ).await
    }

    async fn set_property<T>(&self, prop: &str, value: T, timeout_ms: i32) -> Result<(), Box<dyn Error>>
    where
        T: Arg + Append,
    {
        bluetooth_utils::set_property(
            &self.session.get_connection(),
            ADAPTER_INTERFACE,
            &self.object_path,
            prop,
            value,
            timeout_ms,
        ).await
    }

    async fn call_method<A, R>(
        &self,
        method: &str,
        param: A,
        timeout_ms: i32,
    ) -> Result<R, Box<dyn Error>> 
    where A: AppendAll, R: ReadAll + 'static

    {
        bluetooth_utils::call_method(
            &self.session.get_connection(),
            ADAPTER_INTERFACE,
            &self.object_path,
            method,
            param,
            timeout_ms,
        ).await
    }

    /*
     * Properties
     */

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/adapter-api.txt#n108
    pub async fn get_address(&self) -> Result<String, Box<dyn Error>> {
        self.get_property("Address").await
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/adapter-api.txt#n112
    pub async fn get_name(&self) -> Result<String, Box<dyn Error>> {
        self.get_property("Name").await
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/adapter-api.txt#n120
    pub async fn get_alias(&self) -> Result<String, Box<dyn Error>> {
        self.get_property("Alias").await
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/adapter-api.txt#n120
    pub async fn set_alias(&self, value: &str) -> Result<(), Box<dyn Error>> {
        self.set_property("Alias", value, 1000).await
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/adapter-api.txt#n139
    pub async fn get_class(&self) -> Result<u32, Box<dyn Error>> {
        self.get_property("Class").await
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/adapter-api.txt#n147
    pub async fn is_powered(&self) -> Result<bool, Box<dyn Error>> {
        self.get_property("Powered").await
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/adapter-api.txt#n147
    pub async fn set_powered(&self, value: bool) -> Result<(), Box<dyn Error>> {
        self.set_property("Powered", value, 10000).await
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/adapter-api.txt#n156
    pub async fn is_discoverable(&self) -> Result<bool, Box<dyn Error>> {
        self.get_property("Discoverable").await
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/adapter-api.txt#n156
    pub async fn set_discoverable(&self, value: bool) -> Result<(), Box<dyn Error>> {
        self.set_property("Discoverable", value, 1000).await
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/adapter-api.txt#n176
    pub async fn is_pairable(&self) -> Result<bool, Box<dyn Error>> {
        self.get_property("Pairable").await
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/adapter-api.txt#n176
    pub async fn set_pairable(&self, value: bool) -> Result<(), Box<dyn Error>> {
        self.set_property("Pairable", value, 1000).await
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/adapter-api.txt#n187
    pub async fn get_pairable_timeout(&self) -> Result<u32, Box<dyn Error>> {
        self.get_property("PairableTimeout").await
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/adapter-api.txt#n187
    pub async fn set_pairable_timeout(&self, value: u32) -> Result<(), Box<dyn Error>> {
        self.set_property("PairableTimeout", value, 1000).await
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/adapter-api.txt#n196
    pub async fn get_discoverable_timeout(&self) -> Result<u32, Box<dyn Error>> {
        self.get_property("DiscoverableTimeout").await
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/adapter-api.txt#n196
    pub async fn set_discoverable_timeout(&self, value: u32) -> Result<(), Box<dyn Error>> {
        self.set_property("DiscoverableTimeout", value, 1000).await
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/adapter-api.txt#n205
    pub async fn is_discovering(&self) -> Result<bool, Box<dyn Error>> {
        self.get_property("Discovering").await
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/adapter-api.txt#n209
    pub async fn get_uuids(&self) -> Result<Vec<String>, Box<dyn Error>> {
        self.get_property("UUIDs").await
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/adapter-api.txt#n215
    pub async fn get_modalias(&self) -> Result<(String, u32, u32, u32), Box<dyn Error>> {
        let modalias: String = self.get_property("Modalias").await?;
        let ids: Vec<&str> = modalias.split(':').collect();

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

    pub async fn get_vendor_id_source(&self) -> Result<String, Box<dyn Error>> {
        let (vendor_id_source, _, _, _) = self.get_modalias().await?;
        Ok(vendor_id_source)
    }

    pub async fn get_vendor_id(&self) -> Result<u32, Box<dyn Error>> {
        let (_, vendor_id, _, _) = self.get_modalias().await?;
        Ok(vendor_id)
    }

    pub async fn get_product_id(&self) -> Result<u32, Box<dyn Error>> {
        let (_, _, product_id, _) = self.get_modalias().await?;
        Ok(product_id)
    }

    pub async fn get_device_id(&self) -> Result<u32, Box<dyn Error>> {
        let (_, _, _, device_id) = self.get_modalias().await?;
        Ok(device_id)
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/adapter-api.txt#n12
    // Don't use this method, it's just a bomb now.
    //pub fn start_discovery(&self) -> Result<(), Box<dyn Error>> {
    //    Err(Box::from("Deprecated, use Discovery Session"))
    //}

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/adapter-api.txt#n27
    // Don't use this method, it's just a bomb now.
    //pub fn stop_discovery(&self) -> Result<(), Box<dyn Error>> {
    //    Err(Box::from("Deprecated, use Discovery Session"))
    //}

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/adapter-api.txt#n40
    pub async fn remove_device(&self, device: &str) -> Result<(), Box<dyn Error>> {
        self.call_method(
            "RemoveDevice",
            (Path::from(device),),
            1000,
        ).await?;
        Ok(())
    }

    // http://git.kernel.org/pub/scm/bluetooth/bluez.git/tree/doc/adapter-api.txt#n154
    pub async fn connect_device(
        &self,
        address: &str,
        address_type: AddressType,
        timeout_ms: i32,
    ) -> Result<Path<'static>, Box<dyn Error>> {
        let mut m = HashMap::new();
        m.insert("Address", address);
        m.insert("AddressType", match address_type {
            AddressType::Public => "public",
            AddressType::Random => "random",
        });

        let (path,): (Path,) = self.call_method("ConnectDevice", (m,), timeout_ms).await?;
        Ok(path)
    }
}

pub enum AddressType {
    Public,
    Random,
}
