use dbus::{Connection, BusType, Props};

static SERVICE_NAME: &'static str = "org.bluez";
static DEVICE_INTERFACE: &'static str = "org.bluez.Device1";

#[derive(Clone, Debug)]
pub struct BluetoothDevice {
    object_path: String,
}

impl BluetoothDevice {
    fn new(object_path: String)
           -> BluetoothDevice {
        BluetoothDevice {
            object_path: object_path
        }
    }

    pub fn create_device(object_path: String) -> BluetoothDevice {
        BluetoothDevice::new(object_path)
    }

    pub fn get_object_path(&self) -> String {
        self.object_path.clone()
    }

    pub fn get_address(&self) -> String {
        let c = Connection::get_private(BusType::System).unwrap();
        let d = Props::new(&c, SERVICE_NAME, &self.object_path, DEVICE_INTERFACE, 10000);
        String::from(d.get("Address").unwrap().inner::<&str>().unwrap())
    }

    pub fn get_name(&self) -> String {
        let c = Connection::get_private(BusType::System).unwrap();
        let d = Props::new(&c, SERVICE_NAME, &self.object_path, DEVICE_INTERFACE, 10000);
        String::from(d.get("Name").unwrap().inner::<&str>().unwrap())
    }

    pub fn get_class(&self) -> u32 {
        let c = Connection::get_private(BusType::System).unwrap();
        let d = Props::new(&c, SERVICE_NAME, &self.object_path, DEVICE_INTERFACE, 10000);
        d.get("Class").unwrap().inner::<u32>().unwrap()
    }

    pub fn get_vendor_id(&self) -> u32 {
        let vendor_id = 0u32;
        let modalias = self.get_modalias();
        //TODO get vendor_id
        vendor_id
    }

    pub fn get_product_id(&self) -> u32 {
        let product_id = 0u32;
        let modalias = self.get_modalias();
        //TODO get product_id
        product_id
    }

    pub fn get_product_version(&self) -> u32 {
        let product_id = 0u32;
        let modalias = self.get_modalias();
        //TODO get product_id
        product_id
    }

    fn get_modalias(&self) -> String {
        let c = Connection::get_private(BusType::System).unwrap();
        let d = Props::new(&c, SERVICE_NAME, &self.object_path, DEVICE_INTERFACE, 10000);
        String::from(d.get("Modalias").unwrap().inner::<&str>().unwrap())
    }

}