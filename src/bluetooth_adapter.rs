use dbus::{Connection, BusType, Message, MessageItem};
use bluetooth_utils;
use bluetooth_device::BluetoothDevice;

static ADAPTER_INTERFACE: &'static str = "org.bluez.Adapter1";
static SERVICE_NAME: &'static str = "org.bluez";

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

    pub fn init() -> Result<BluetoothAdapter,String> {
        let adapters = bluetooth_utils::get_adapters();

        if adapters.is_empty() {
            return Err(String::from("Bluetooth adapter not found"))
        }

        Ok(BluetoothAdapter::new(adapters[0].clone()))
    }

    pub fn get_object_path(&self) -> String {
        self.object_path.clone()
    }

    fn get_property(&self, prop: &str) -> Result<MessageItem, String> {
        bluetooth_utils::get_property(ADAPTER_INTERFACE, &self.object_path, prop)
    }

    fn set_property<T>(&self, prop: &str, value: T) -> Result<(), String>
    where T: Into<MessageItem> {
        bluetooth_utils::set_property(ADAPTER_INTERFACE, &self.object_path, prop, value)
    }

    pub fn get_address(&self) -> Result<String, String> {
        match self.get_property("Address") {
            Ok(address) => Ok(String::from(address.inner::<&str>().unwrap())),
            Err(e) => Err(e),
        }
    }

    pub fn get_name(&self) -> Result<String, String> {
        match self.get_property("Name") {
            Ok(name) => Ok(String::from(name.inner::<&str>().unwrap())),
            Err(e) => Err(e),
        }
    }

    pub fn get_alias(&self) -> Result<String, String> {
        match self.get_property("Alias") {
            Ok(alias) => Ok(String::from(alias.inner::<&str>().unwrap())),
            Err(e) => Err(e),
        }
    }

    pub fn set_alias(&self, value: String) -> Result<(),String> {
        match self.set_property("Alias", value) {
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        }
    }

    pub fn get_class(&self) -> Result<u32, String> {
        match self.get_property("Class") {
            Ok(class) => Ok(class.inner::<u32>().unwrap()),
            Err(e) => Err(e),
        }
    }

    pub fn is_powered(&self) -> Result<bool, String> {
        match self.get_property("Powered") {
            Ok(powered) => Ok(powered.inner::<bool>().unwrap()),
            Err(e) => Err(e),
        }
    }

    pub fn set_powered(&self, value: bool) -> Result<(),String> {
        match self.set_property("Powered", value) {
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        }
    }

    pub fn get_rssi(&self) -> Result<i32, String> {
        match self.get_property("RSSI") {
            Ok(rssi) => Ok(rssi.inner::<i32>().unwrap()),
            Err(e) => Err(e),
        }
    }

    pub fn is_discoverable(&self) -> Result<bool, String> {
        match self.get_property("Discoverable") {
            Ok(discoverable) => Ok(discoverable.inner::<bool>().unwrap()),
            Err(e) => Err(e),
        }
    }

    pub fn set_discoverable(&self, value: bool) -> Result<(),String> {
        match self.set_property("Discoverable", value) {
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        }
    }

    pub fn is_discovering(&self) -> Result<bool, String> {
        match self.get_property("Discovering") {
            Ok(discovering) => Ok(discovering.inner::<bool>().unwrap()),
            Err(e) => Err(e),
        }
    }


    pub fn get_first_device(&self) -> Result<BluetoothDevice, String> {
        let devices = bluetooth_utils::list_devices(&self.object_path);

        if devices.is_empty() {
            return Err(String::from("No device found."))
        }
        Ok(BluetoothDevice::create_device(devices[0].clone()))
    }

    pub fn get_device_list(&self) -> Vec<String> {
        bluetooth_utils::list_devices(&self.object_path)
    }

    pub fn start_discovery(&self) -> Result<(), String> {
        let c = match Connection::get_private(BusType::System) {
            Ok(conn) => conn,
            Err(_) => return Err(String::from("Error! Connecting to dbus."))
        };
        let m = Message::new_method_call(SERVICE_NAME, &self.object_path, ADAPTER_INTERFACE, "StartDiscovery").unwrap();
        match c.send(m) {
            Ok(_) => Ok(()),
            Err(_) => Err(String::from("Error! Starting discovery.")),
        }
    }

    pub fn stop_discovery(&self) -> Result<(), String> {
        let c = match Connection::get_private(BusType::System) {
            Ok(conn) => conn,
            Err(_) => return Err(String::from("Error! Connecting to dbus."))
        };
        let m = Message::new_method_call(SERVICE_NAME, &self.object_path, ADAPTER_INTERFACE, "StopDiscovery").unwrap();
        match c.send(m) {
            Ok(_) => Ok(()),
            Err(_) => Err(String::from("Error! Starting discovery.")),
        }
    }

}
