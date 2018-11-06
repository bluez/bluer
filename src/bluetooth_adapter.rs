use bluetooth_device::BluetoothDevice;
use bluetooth_session::BluetoothSession;
use bluetooth_utils;
use dbus::{MessageItem, MessageItemArray, Signature};
use hex::FromHex;
use std::error::Error;

static ADAPTER_INTERFACE: &'static str = "org.bluez.Adapter1";

#[derive(Clone)]
pub struct BluetoothAdapter<'a> {
    object_path: String,
    session: &'a BluetoothSession,
}

impl<'a> BluetoothAdapter<'a> {
    fn new(session: &'a BluetoothSession, object_path: String) -> BluetoothAdapter<'a> {
        BluetoothAdapter {
            object_path: object_path,
            session: session,
        }
    }

    pub fn init(session: &BluetoothSession) -> Result<BluetoothAdapter, Box<Error>> {
        let adapters = try!(bluetooth_utils::get_adapters(session.get_connection()));

        if adapters.is_empty() {
            return Err(Box::from("Bluetooth adapter not found"));
        }

        Ok(BluetoothAdapter::new(session, adapters[0].clone()))
    }

    pub fn create_adapter(
        session: &BluetoothSession,
        object_path: String,
    ) -> Result<BluetoothAdapter, Box<Error>> {
        let adapters = try!(bluetooth_utils::get_adapters(session.get_connection()));

        for adapter in adapters {
            if adapter == object_path {
                return Ok(BluetoothAdapter::new(session, adapter.clone()));
            }
        }
        Err(Box::from("Bluetooth adapter not found"))
    }

    pub fn get_id(&self) -> String {
        self.object_path.clone()
    }

    pub fn get_first_device(&self) -> Result<BluetoothDevice, Box<Error>> {
        let devices = try!(bluetooth_utils::list_devices(
            self.session.get_connection(),
            &self.object_path
        ));

        if devices.is_empty() {
            return Err(Box::from("No device found."));
        }
        Ok(BluetoothDevice::new(self.session, devices[0].clone()))
    }

    pub fn get_device_list(&self) -> Result<Vec<String>, Box<Error>> {
        bluetooth_utils::list_devices(self.session.get_connection(), &self.object_path)
    }

    fn get_property(&self, prop: &str) -> Result<MessageItem, Box<Error>> {
        bluetooth_utils::get_property(
            self.session.get_connection(),
            ADAPTER_INTERFACE,
            &self.object_path,
            prop,
        )
    }

    fn set_property<T>(&self, prop: &str, value: T, timeout_ms: i32) -> Result<(), Box<Error>>
    where
        T: Into<MessageItem>,
    {
        bluetooth_utils::set_property(
            self.session.get_connection(),
            ADAPTER_INTERFACE,
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
    ) -> Result<(), Box<Error>> {
        bluetooth_utils::call_method(
            self.session.get_connection(),
            ADAPTER_INTERFACE,
            &self.object_path,
            method,
            param,
            timeout_ms,
        )
    }

    /*
     * Properties
     */

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/adapter-api.txt#n108
    pub fn get_address(&self) -> Result<String, Box<Error>> {
        let address = try!(self.get_property("Address"));
        Ok(String::from(address.inner::<&str>().unwrap()))
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/adapter-api.txt#n112
    pub fn get_name(&self) -> Result<String, Box<Error>> {
        let name = try!(self.get_property("Name"));
        Ok(String::from(name.inner::<&str>().unwrap()))
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/adapter-api.txt#n120
    pub fn get_alias(&self) -> Result<String, Box<Error>> {
        let alias = try!(self.get_property("Alias"));
        Ok(String::from(alias.inner::<&str>().unwrap()))
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/adapter-api.txt#n120
    pub fn set_alias(&self, value: String) -> Result<(), Box<Error>> {
        self.set_property("Alias", value, 1000)
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/adapter-api.txt#n139
    pub fn get_class(&self) -> Result<u32, Box<Error>> {
        let class = try!(self.get_property("Class"));
        Ok(class.inner::<u32>().unwrap())
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/adapter-api.txt#n147
    pub fn is_powered(&self) -> Result<bool, Box<Error>> {
        let powered = try!(self.get_property("Powered"));
        Ok(powered.inner::<bool>().unwrap())
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/adapter-api.txt#n147
    pub fn set_powered(&self, value: bool) -> Result<(), Box<Error>> {
        self.set_property("Powered", value, 10000)
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/adapter-api.txt#n156
    pub fn is_discoverable(&self) -> Result<bool, Box<Error>> {
        let discoverable = try!(self.get_property("Discoverable"));
        Ok(discoverable.inner::<bool>().unwrap())
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/adapter-api.txt#n156
    pub fn set_discoverable(&self, value: bool) -> Result<(), Box<Error>> {
        self.set_property("Discoverable", value, 1000)
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/adapter-api.txt#n176
    pub fn is_pairable(&self) -> Result<bool, Box<Error>> {
        let pairable = try!(self.get_property("Pairable"));
        Ok(pairable.inner::<bool>().unwrap())
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/adapter-api.txt#n176
    pub fn set_pairable(&self, value: bool) -> Result<(), Box<Error>> {
        self.set_property("Pairable", value, 1000)
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/adapter-api.txt#n187
    pub fn get_pairable_timeout(&self) -> Result<u32, Box<Error>> {
        let pairable_timeout = try!(self.get_property("PairableTimeout"));
        Ok(pairable_timeout.inner::<u32>().unwrap())
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/adapter-api.txt#n187
    pub fn set_pairable_timeout(&self, value: u32) -> Result<(), Box<Error>> {
        self.set_property("PairableTimeout", value, 1000)
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/adapter-api.txt#n196
    pub fn get_discoverable_timeout(&self) -> Result<u32, Box<Error>> {
        let discoverable_timeout = try!(self.get_property("DiscoverableTimeout"));
        Ok(discoverable_timeout.inner::<u32>().unwrap())
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/adapter-api.txt#n196
    pub fn set_discoverable_timeout(&self, value: u32) -> Result<(), Box<Error>> {
        self.set_property("DiscoverableTimeout", value, 1000)
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/adapter-api.txt#n205
    pub fn is_discovering(&self) -> Result<bool, Box<Error>> {
        let discovering = try!(self.get_property("Discovering"));
        Ok(discovering.inner::<bool>().unwrap())
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/adapter-api.txt#n209
    pub fn get_uuids(&self) -> Result<Vec<String>, Box<Error>> {
        let uuids = try!(self.get_property("UUIDs"));
        let z: &[MessageItem] = uuids.inner().unwrap();
        let mut v: Vec<String> = Vec::new();
        for y in z {
            v.push(String::from(y.inner::<&str>().unwrap()));
        }
        Ok(v)
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/adapter-api.txt#n215
    pub fn get_modalias(&self) -> Result<(String, u32, u32, u32), Box<Error>> {
        let modalias = try!(self.get_property("Modalias"));
        let m = modalias.inner::<&str>().unwrap();
        let ids: Vec<&str> = m.split(":").collect();

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

    pub fn get_vendor_id_source(&self) -> Result<String, Box<Error>> {
        let (vendor_id_source, _, _, _) = try!(self.get_modalias());
        Ok(vendor_id_source)
    }

    pub fn get_vendor_id(&self) -> Result<u32, Box<Error>> {
        let (_, vendor_id, _, _) = try!(self.get_modalias());
        Ok(vendor_id)
    }

    pub fn get_product_id(&self) -> Result<u32, Box<Error>> {
        let (_, _, product_id, _) = try!(self.get_modalias());
        Ok(product_id)
    }

    pub fn get_device_id(&self) -> Result<u32, Box<Error>> {
        let (_, _, _, device_id) = try!(self.get_modalias());
        Ok(device_id)
    }

    /*
     * Methods
     */

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/adapter-api.txt#n12
    pub fn start_discovery(&self) -> Result<(), Box<Error>> {
        Err(Box::from("Deprecated, use Discovery Session"))
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/adapter-api.txt#n27
    pub fn stop_discovery(&self) -> Result<(), Box<Error>> {
        Err(Box::from("Deprecated, use Discovery Session"))
    }

    // http://git.kernel.org/cgit/bluetooth/bluez.git/tree/doc/adapter-api.txt#n40
    pub fn remove_device(&self, device: String) -> Result<(), Box<Error>> {
        self.call_method(
            "RemoveDevice",
            Some(&[MessageItem::ObjectPath(device.into())]),
            1000,
        )
    }

    // http://git.kernel.org/pub/scm/bluetooth/bluez.git/tree/doc/adapter-api.txt#n154
    pub fn connect_device(
        &self,
        address: String,
        address_type: AddressType,
        timeout_ms: i32,
    ) -> Result<(), Box<Error>> {
        let address_type = match address_type {
            AddressType::Public => "public",
            AddressType::Random => "random",
        };

        let mut m = vec![MessageItem::DictEntry(
            Box::new("Address".into()),
            Box::new(MessageItem::Variant(Box::new(address.into()))),
        )];

        m.push(MessageItem::DictEntry(
            Box::new("AddressType".into()),
            Box::new(MessageItem::Variant(Box::new(address_type.into()))),
        ));

        self.call_method(
            "ConnectDevice",
            Some(&[MessageItem::Array(
                MessageItemArray::new(m, Signature::from("a{sv}")).unwrap(),
            )]),
            timeout_ms,
        )
    }
}

pub enum AddressType {
    Public,
    Random,
}
