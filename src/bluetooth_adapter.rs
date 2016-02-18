use dbus::{Connection, BusType, Props};
use bluetooth_utils;
use bluetooth_device::BluetoothDevice;

#[allow(dead_code)]
static ADAPTER_INTERFACE: &'static str = "org.bluez.Adapter1";
static SERVICE_NAME: &'static str = "org.bluez";
static DEVICE_INTERFACE: &'static str = "org.bluez.Device1";

#[derive(Debug)]
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

    pub fn get_first_device(&self) -> Result<BluetoothDevice, String> {
        let devices = bluetooth_utils::list_devices(&self.object_path);

        if devices.is_empty() {
            return Err(String::from("No device found."))
        }

        let c = Connection::get_private(BusType::System).unwrap();
        let d = Props::new(&c, SERVICE_NAME, &devices[0], DEVICE_INTERFACE, 10000);
        Ok(BluetoothDevice::new(String::from(d.get("Address").unwrap().inner::<&str>().unwrap()),
                                String::from(d.get("Name").unwrap().inner::<&str>().unwrap()),
                                d.get("Class").unwrap().inner::<u32>().unwrap(),
                                0u32,
                                0u32,
                                0u32,
                                None))
    }
}
