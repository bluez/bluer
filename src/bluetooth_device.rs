use dbus::{Connection, BusType, Message, MessageItem};
use bluetooth_utils;
use rustc_serialize::hex::FromHex;

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

    fn get_property(&self, prop: &str) -> Result<MessageItem, String> {
        bluetooth_utils::get_property(DEVICE_INTERFACE, &self.object_path, prop)
    }

    fn set_property<T>(&self, prop: &str, value: T) -> Result<(), String>
    where T: Into<MessageItem> {
        bluetooth_utils::set_property(DEVICE_INTERFACE, &self.object_path, prop, value)
    }

/*
 * Properties
 */

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

    pub fn get_vendor_id(&self) -> Result<u32, String> {
        match self.get_modalias() {
            Ok((_,vendor_id,_,_)) => Ok(vendor_id),
            Err(e) => Err(e),
        }
    }

    pub fn get_product_id(&self) -> Result<u32, String> {
        match self.get_modalias() {
            Ok((_,_,product_id,_)) => Ok(product_id),
            Err(e) => Err(e),
        }
    }

    pub fn get_device_id(&self) -> Result<u32, String> {
        match self.get_modalias() {
            Ok((_,_,_,device_id)) => Ok(device_id),
            Err(e) => Err(e),
        }
    }

    fn get_modalias(&self) ->  Result<(String, u32, u32, u32), String> {
        let modalias = match self.get_property("Modalias") {
            Ok(m) => m,
            Err(e) => return Err(e),
        };
        let m = modalias.inner::<&str>().unwrap();
        let ids: Vec<&str> = m.split(":").collect();

        let source = String::from(ids[0]);
        let vendor = ids[1][1..5].from_hex().unwrap();
        let product = ids[1][6..10].from_hex().unwrap();
        let device = ids[1][11..15].from_hex().unwrap();

        Ok((source,
        (vendor[0] as u32) * 16 * 16 + (vendor[1] as u32),
        (product[0] as u32) * 16 * 16 + (product[1] as u32),
        (device[0] as u32) * 16 * 16 + (device[1] as u32)))
    }

    pub fn is_pairable(&self) -> Result<bool, String> {
        match self.get_property("Pairable") {
            Ok(pairable) => Ok(pairable.inner::<bool>().unwrap()),
            Err(e) => Err(e),
        }
    }

    pub fn is_paired(&self) -> Result<bool, String> {
         match self.get_property("Paired") {
            Ok(paired) => Ok(paired.inner::<bool>().unwrap()),
            Err(e) => Err(e),
        }
    }

    pub fn is_connectable(&self) -> Result<bool, String> {
         match self.get_property("Connectable") {
            Ok(connectable) => Ok(connectable.inner::<bool>().unwrap()),
            Err(e) => Err(e),
        }
    }

    pub fn is_connected(&self) -> Result<bool, String> {
         match self.get_property("Connected") {
            Ok(connected) => Ok(connected.inner::<bool>().unwrap()),
            Err(e) => Err(e),
        }
    }

    pub fn is_trustable(&self) -> Result<bool, String> {
        match self.get_property("Trustable") {
            Ok(trustable) => Ok(trustable.inner::<bool>().unwrap()),
            Err(e) => Err(e),
        }
    }

    pub fn get_uuids(&self) -> Result<Vec<String>, String> {
        let uuids = match self.get_property("UUIDs") {
            Ok(u) => u,
            Err(e) => return Err(e),
        };
        let z: &[MessageItem] = uuids.inner().unwrap();
        let mut v: Vec<String> = Vec::new();
        for y in z {
            v.push(String::from(y.inner::<&str>().unwrap()));
        }
        Ok(v)
    }

    pub fn get_gatt_services(&self) -> Result<Vec<String>,String> {
        let services = match self.get_property("GattServices") {
            Ok(s) => s,
            Err(e) => return Err(e),
        };
        let z: &[MessageItem] = services.inner().unwrap();
        let mut v: Vec<String> = Vec::new();
        for y in z {
            v.push(String::from(y.inner::<&str>().unwrap()));
        }
        Ok(v)
    }

/*
 * Methods
 */

    pub fn connect(&self) -> Result<(), String> {
        let c = match Connection::get_private(BusType::System) {
            Ok(conn) => conn,
            Err(_) => return Err(String::from("Error! Connecting to dbus."))
        };
        let m = Message::new_method_call(SERVICE_NAME, &self.object_path, DEVICE_INTERFACE, "Connect").unwrap();
        match c.send_with_reply_and_block(m, 15000) {
            Ok(_) => Ok(()),
            Err(_) => Err(String::from("Error! Connecting.")),
        }
    }

    pub fn disconnect(&self) -> Result<(), String>{
        let c = match Connection::get_private(BusType::System) {
            Ok(conn) => conn,
            Err(_) => return Err(String::from("Error! Connecting to dbus."))
        };
        let m = Message::new_method_call(SERVICE_NAME, &self.object_path, DEVICE_INTERFACE, "Disconnect").unwrap();
        match c.send_with_reply_and_block(m, 15000) {
            Ok(_) => Ok(()),
            Err(_) => Err(String::from("Error! Disconnecting.")),
        }
    }

    

}