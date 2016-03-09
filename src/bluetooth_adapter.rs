use dbus::MessageItem;
use bluetooth_device::BluetoothDevice;
use bluetooth_utils;

use std::error::Error;

static ADAPTER_INTERFACE: &'static str = "org.bluez.Adapter1";

#[derive(Clone, Debug)]
pub struct BluetoothAdapter {
    object_path: String,
}

impl BluetoothAdapter {
    fn new(object_path: String) -> BluetoothAdapter {
        BluetoothAdapter {
            object_path: object_path,
        }
    }

    pub fn init() -> Result<BluetoothAdapter, Box<Error>> {
        let adapters = try!(bluetooth_utils::get_adapters());

        if adapters.is_empty() {
            return Err(Box::from("Bluetooth adapter not found"))
        }

        Ok(BluetoothAdapter::new(adapters[0].clone()))
    }

    pub fn get_object_path(&self) -> String {
        self.object_path.clone()
    }

    pub fn get_first_device(&self) -> Result<BluetoothDevice, Box<Error>> {
        let devices = try!(bluetooth_utils::list_devices(&self.object_path));

        if devices.is_empty() {
            return Err(Box::from("No device found."))
        }
        Ok(BluetoothDevice::create_device(devices[0].clone()))
    }

    pub fn get_device_list(&self) -> Result<Vec<String>, Box<Error>> {
        bluetooth_utils::list_devices(&self.object_path)
    }

    fn get_property(&self, prop: &str) -> Result<MessageItem, Box<Error>> {
        bluetooth_utils::get_property(ADAPTER_INTERFACE, &self.object_path, prop)
    }

    fn set_property<T>(&self, prop: &str, value: T) -> Result<(), Box<Error>>
    where T: Into<MessageItem> {
        bluetooth_utils::set_property(ADAPTER_INTERFACE, &self.object_path, prop, value)
    }

    fn call_method(&self, method: &str, param: Option<[MessageItem; 1]>) -> Result<(), Box<Error>> {
        bluetooth_utils::call_method(ADAPTER_INTERFACE, &self.object_path, method, param)
    }

/*
 * Properties
 */

    pub fn get_address(&self) -> Result<String, Box<Error>> {
        let address = try!(self.get_property("Address"));
        Ok(String::from(address.inner::<&str>().unwrap()))
    }

    pub fn get_name(&self) -> Result<String, Box<Error>> {
        let name = try!(self.get_property("Name"));
        Ok(String::from(name.inner::<&str>().unwrap()))
    }

    pub fn get_alias(&self) -> Result<String, Box<Error>> {
        let alias = try!(self.get_property("Alias"));
        Ok(String::from(alias.inner::<&str>().unwrap()))
    }

    pub fn set_alias(&self, value: String) -> Result<(), Box<Error>> {
        self.set_property("Alias", value)
    }

    pub fn get_class(&self) -> Result<u32, Box<Error>> {
        let class = try!(self.get_property("Class"));
        Ok(class.inner::<u32>().unwrap())
    }

    pub fn is_powered(&self) -> Result<bool, Box<Error>> {
        let powered = try!(self.get_property("Powered"));
        Ok(powered.inner::<bool>().unwrap())
    }

    pub fn set_powered(&self, value: bool) -> Result<(),Box<Error>> {
        self.set_property("Powered", value)
    }

    pub fn get_rssi(&self) -> Result<i32, Box<Error>> {
        let rssi = try!(self.get_property("RSSI"));
        Ok(rssi.inner::<i32>().unwrap())
    }

    pub fn is_discoverable(&self) -> Result<bool, Box<Error>> {
        let discoverable = try!(self.get_property("Discoverable"));
        Ok(discoverable.inner::<bool>().unwrap())
    }

    pub fn set_discoverable(&self, value: bool) -> Result<(), Box<Error>> {
        self.set_property("Discoverable", value)
    }

    pub fn is_discovering(&self) -> Result<bool, Box<Error>> {
        let discovering = try!(self.get_property("Discovering"));
        Ok(discovering.inner::<bool>().unwrap())
    }

/*
 * Methods
 */

    pub fn start_discovery(&self) -> Result<(), Box<Error>> {
        self.call_method("StartDiscovery", None)
    }

    pub fn stop_discovery(&self) -> Result<(), Box<Error>> {
        self.call_method("StopDiscovery", None)
    }
}
