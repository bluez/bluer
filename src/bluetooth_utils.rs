use dbus::{Connection, BusType, Message, MessageItem, Props};
use std::error::Error;

static ADAPTER_INTERFACE: &'static str = "org.bluez.Adapter1";
static DEVICE_INTERFACE: &'static str = "org.bluez.Device1";
static SERVICE_INTERFACE: &'static str = "org.bluez.GattService1";
static CHARACTERISTIC_INTERFACE: &'static str = "org.bluez.GattCharacteristic1";
static DESCRIPTOR_INTERFACE: &'static str = "org.bluez.GattDescriptor1";
static SERVICE_NAME: &'static str = "org.bluez";

fn get_managed_objects(c: &Connection) ->  Result<Vec<MessageItem>, Box<Error>> {
    let m = try!(Message::new_method_call(SERVICE_NAME, "/", "org.freedesktop.DBus.ObjectManager", "GetManagedObjects"));
    let r = try!(c.send_with_reply_and_block(m, 1000));
    Ok(r.get_items())
}

pub fn get_adapters() -> Result<Vec<String>, Box<Error>> {
    let mut adapters: Vec<String> = Vec::new();
    let c = try!(Connection::get_private(BusType::System));
    let objects: Vec<MessageItem> = try!(get_managed_objects(&c));
    let z: &[MessageItem] = objects.get(0).unwrap().inner().unwrap();
    for y in z {
        let (path, interfaces) = y.inner().unwrap();
        let x: &[MessageItem] = interfaces.inner().unwrap();
        for interface in x {
            let (i,_) = interface.inner().unwrap();
            let name: &str = i.inner().unwrap();
            if name == ADAPTER_INTERFACE {
                let p: &str = path.inner().unwrap();
                adapters.push(String::from(p));
            }
        }
    }
    Ok(adapters)
}

pub fn list_devices(adapter_path: &String) -> Result<Vec<String>, Box<Error>> {
    list_item(DEVICE_INTERFACE, adapter_path, "Adapter")
}

pub fn list_services(device_path: &String) -> Result<Vec<String>, Box<Error>> {
    list_item(SERVICE_INTERFACE, device_path, "Device")
}

pub fn list_characteristics(device_path: &String) -> Result<Vec<String>, Box<Error>> {
    list_item(CHARACTERISTIC_INTERFACE, device_path, "Service")
}

pub fn list_descriptors(device_path: &String) -> Result<Vec<String>, Box<Error>> {
    list_item(DESCRIPTOR_INTERFACE, device_path, "Characteristic")
}

fn list_item(item_interface: &str, item_path: &str, item_property: &str) -> Result<Vec<String>, Box<Error>> {
    let mut v: Vec<String> = Vec::new();
    let c = try!(Connection::get_private(BusType::System));
    let objects: Vec<MessageItem> = try!(get_managed_objects(&c));
    let z: &[MessageItem] = objects.get(0).unwrap().inner().unwrap();
    for y in z {
        let (path, interfaces) = y.inner().unwrap();
        let x: &[MessageItem] = interfaces.inner().unwrap();
        for interface in x {
            let (i,_) = interface.inner().unwrap();
            let name: &str = i.inner().unwrap();
            if name == item_interface {
                let objpath: &str = path.inner().unwrap();
                let prop = try!(get_property(item_interface, objpath, item_property));
                let prop_path = prop.inner::<&str>().unwrap();
                if prop_path == item_path {
                    v.push(String::from(objpath));
                }
            }
        }
    }
    Ok(v)
}

pub fn get_property(interface: &str, object_path: &str, prop: &str) -> Result<MessageItem, Box<Error>> {
    let c = try!(Connection::get_private(BusType::System));
    let p = Props::new(&c, SERVICE_NAME, object_path, interface, 1000);
    Ok(try!(p.get(prop)).clone())
}

pub fn set_property<T>(interface: &str, object_path: &str, prop: &str, value: T) -> Result<(), Box<Error>>
where T: Into<MessageItem> {
    let c = try!(Connection::get_private(BusType::System));
    let p = Props::new(&c, SERVICE_NAME, object_path, interface, 1000);
    Ok(try!(p.set(prop, value.into())))
}

pub fn call_method(interface: &str, object_path: &str, method: &str, param: Option<&[MessageItem]>) -> Result<(), Box<Error>> {
    let c = try!(Connection::get_private(BusType::System));
    let mut m = try!(Message::new_method_call(SERVICE_NAME, object_path, interface, method));
    match param {
        Some(p) => m.append_items(p),
        None => (),
    };
    try!(c.send_with_reply_and_block(m, 1000));
    Ok(())
}
