use dbus::{Connection, BusType, Props, Message, MessageItem};

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

    pub fn get_alias(&self) -> String {
        let c = Connection::get_private(BusType::System).unwrap();
        let d = Props::new(&c, SERVICE_NAME, &self.object_path, DEVICE_INTERFACE, 10000);
        String::from(d.get("Alias").unwrap().inner::<&str>().unwrap())
    }

    pub fn set_alias(&self, value: String) {
        let c = Connection::get_private(BusType::System).unwrap();
        let d = Props::new(&c, SERVICE_NAME, &self.object_path, DEVICE_INTERFACE, 10000);
        d.set("Alias", value.into()).unwrap();
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

    pub fn get_device_id(&self) -> u32 {
        let device_id = 0u32;
        let modalias = self.get_modalias();
        //TODO get device_id
        device_id
    }

    fn get_modalias(&self) -> String {
        let c = Connection::get_private(BusType::System).unwrap();
        let d = Props::new(&c, SERVICE_NAME, &self.object_path, DEVICE_INTERFACE, 10000);
        String::from(d.get("Modalias").unwrap().inner::<&str>().unwrap())
    }

    pub fn is_pairable(&self) -> bool {
        let c = Connection::get_private(BusType::System).unwrap();
        let d = Props::new(&c, SERVICE_NAME, &self.object_path, DEVICE_INTERFACE, 10000);
        d.get("Pairable").unwrap().inner::<bool>().unwrap()
    }

    pub fn is_paired(&self) -> bool {
        let c = Connection::get_private(BusType::System).unwrap();
        let d = Props::new(&c, SERVICE_NAME, &self.object_path, DEVICE_INTERFACE, 10000);
        d.get("Paired").unwrap().inner::<bool>().unwrap()
    }

    pub fn is_connectable(&self) -> bool {
        let c = Connection::get_private(BusType::System).unwrap();
        let d = Props::new(&c, SERVICE_NAME, &self.object_path, DEVICE_INTERFACE, 10000);
        d.get("Connectable").unwrap().inner::<bool>().unwrap()
    }

    pub fn is_connected(&self) -> bool {
        let c = Connection::get_private(BusType::System).unwrap();
        let d = Props::new(&c, SERVICE_NAME, &self.object_path, DEVICE_INTERFACE, 10000);
        d.get("Connected").unwrap().inner::<bool>().unwrap()
    }

    pub fn is_trustable(&self) -> bool {
        let c = Connection::get_private(BusType::System).unwrap();
        let d = Props::new(&c, SERVICE_NAME, &self.object_path, DEVICE_INTERFACE, 10000);
        d.get("Trustable").unwrap().inner::<bool>().unwrap()
    }

    pub fn get_uuids(&self) -> Vec<String> {
        let c = Connection::get_private(BusType::System).unwrap();
        let d = Props::new(&c, SERVICE_NAME, &self.object_path, DEVICE_INTERFACE, 10000);
        let uuids = d.get("UUIDs").unwrap();
        let z: &[MessageItem] = uuids.inner().unwrap();
        let mut v: Vec<String> = Vec::new();
        for y in z {
            v.push(String::from(y.inner::<&str>().unwrap()));
        }
        v
    }

    pub fn get_gatt_services(&self) -> Vec<String> {
        let c = Connection::get_private(BusType::System).unwrap();
        let d = Props::new(&c, SERVICE_NAME, &self.object_path, DEVICE_INTERFACE, 10000);
        let services = d.get("GattServices").unwrap();
        let z: &[MessageItem] = services.inner().unwrap();
        let mut v: Vec<String> = Vec::new();
        for y in z {
            v.push(String::from(y.inner::<&str>().unwrap()));
        }
        v
    }

    pub fn get_all_properties(&self) {
        let c = Connection::get_private(BusType::System).unwrap();
        let d = Props::new(&c, SERVICE_NAME, &self.object_path, DEVICE_INTERFACE, 10000);
        println!("{:?}", d.get_all().unwrap() );
    }

/*
 * METHOD_CALLS
 */

    pub fn connect(&self) {
        let c = Connection::get_private(BusType::System).unwrap();
        let m = Message::new_method_call(SERVICE_NAME, &self.object_path, DEVICE_INTERFACE, "Connect").unwrap();
        let r = c.send_with_reply_and_block(m, 15000).unwrap();
    }

    pub fn disconnect(&self) {
        let c = Connection::get_private(BusType::System).unwrap();
        let m = Message::new_method_call(SERVICE_NAME, &self.object_path, DEVICE_INTERFACE, "Disconnect").unwrap();
        let r = c.send_with_reply_and_block(m, 15000).unwrap();
    }

    

}