use dbus::MessageItem;
use bluetooth_utils;
use rustc_serialize::hex::FromHex;

use std::error::Error;

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

    fn get_property(&self, prop: &str) -> Result<MessageItem, Box<Error>> {
        bluetooth_utils::get_property(DEVICE_INTERFACE, &self.object_path, prop)
    }

    fn set_property<T>(&self, prop: &str, value: T) -> Result<(), Box<Error>>
    where T: Into<MessageItem> {
        bluetooth_utils::set_property(DEVICE_INTERFACE, &self.object_path, prop, value)
    }

    fn call_method(&self, method: &str, param: Option<[MessageItem; 1]>) -> Result<(), Box<Error>> {
        bluetooth_utils::call_method(DEVICE_INTERFACE, &self.object_path, method, param)
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

    pub fn set_alias(&self, value: String) -> Result<(),Box<Error>> {
        self.set_property("Alias", value)
    }

    pub fn get_class(&self) -> Result<u32, Box<Error>> {
        let class = try!(self.get_property("Class"));
        Ok(class.inner::<u32>().unwrap())
    }

    pub fn get_vendor_id(&self) -> Result<u32, Box<Error>> {
        let (_,vendor_id,_,_) = try!(self.get_modalias());
        Ok(vendor_id)
    }

    pub fn get_product_id(&self) -> Result<u32, Box<Error>> {
        let (_,_,product_id,_) = try!(self.get_modalias());
        Ok(product_id)
    }

    pub fn get_device_id(&self) -> Result<u32, Box<Error>> {
        let (_,_,_,device_id) = try!(self.get_modalias());
        Ok(device_id)
    }

    fn get_modalias(&self) ->  Result<(String, u32, u32, u32), Box<Error>> {
        let modalias = try!(self.get_property("Modalias"));
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

    pub fn is_pairable(&self) -> Result<bool, Box<Error>> {
        let pairable = try!(self.get_property("Pairable"));
        Ok(pairable.inner::<bool>().unwrap())
    }

    pub fn is_paired(&self) -> Result<bool, Box<Error>> {
         let paired = try!(self.get_property("Paired"));
         Ok(paired.inner::<bool>().unwrap())
    }

    pub fn is_connectable(&self) -> Result<bool, Box<Error>> {
         let connectable = try!(self.get_property("Connectable"));
         Ok(connectable.inner::<bool>().unwrap())
    }

    pub fn is_connected(&self) -> Result<bool, Box<Error>> {
         let connected = try!(self.get_property("Connected"));
         Ok(connected.inner::<bool>().unwrap())
    }

    pub fn is_trustable(&self) -> Result<bool, Box<Error>> {
        let trustable = try!(self.get_property("Trustable"));
        Ok(trustable.inner::<bool>().unwrap())
    }

    pub fn get_uuids(&self) -> Result<Vec<String>, Box<Error>> {
        let uuids = try!(self.get_property("UUIDs"));
        let z: &[MessageItem] = uuids.inner().unwrap();
        let mut v: Vec<String> = Vec::new();
        for y in z {
            v.push(String::from(y.inner::<&str>().unwrap()));
        }
        Ok(v)
    }

    pub fn get_gatt_services(&self) -> Result<Vec<String>, Box<Error>> {
        let services = try!(self.get_property("GattServices"));
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

    pub fn connect(&self) -> Result<(), Box<Error>> {
        self.call_method("Connect", None)
    }

    pub fn disconnect(&self) -> Result<(), Box<Error>>{
        self.call_method("Disconnect", None)
    }
}