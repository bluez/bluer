use dbus::{Connection, Message, MessageItem, Props};
use std::error::Error;

static ADAPTER_INTERFACE: &str = "org.bluez.Adapter1";
static DEVICE_INTERFACE: &str = "org.bluez.Device1";
static SERVICE_INTERFACE: &str = "org.bluez.GattService1";
static CHARACTERISTIC_INTERFACE: &str = "org.bluez.GattCharacteristic1";
static DESCRIPTOR_INTERFACE: &str = "org.bluez.GattDescriptor1";
static SERVICE_NAME: &str = "org.bluez";

fn get_managed_objects(c: &Connection) -> Result<Vec<MessageItem>, Box<dyn Error>> {
    let m = Message::new_method_call(
        SERVICE_NAME,
        "/",
        "org.freedesktop.DBus.ObjectManager",
        "GetManagedObjects"
    )?;
    let r = c.send_with_reply_and_block(m, 1000)?;
    Ok(r.get_items())
}

pub fn get_adapters(c: &Connection) -> Result<Vec<String>, Box<dyn Error>> {
    let mut adapters: Vec<String> = Vec::new();
    let objects: Vec<MessageItem> = get_managed_objects(&c)?;
    let z: &[MessageItem] = objects.get(0).unwrap().inner().unwrap();
    for y in z {
        let (path, interfaces) = y.inner().unwrap();
        let x: &[MessageItem] = interfaces.inner().unwrap();
        for interface in x {
            let (i, _) = interface.inner().unwrap();
            let name: &str = i.inner().unwrap();
            if name == ADAPTER_INTERFACE {
                let p: &str = path.inner().unwrap();
                adapters.push(String::from(p));
            }
        }
    }
    Ok(adapters)
}

pub fn list_devices(c: &Connection, adapter_path: &str) -> Result<Vec<String>, Box<dyn Error>> {
    list_item(c, DEVICE_INTERFACE, adapter_path, "Adapter")
}

pub fn list_services(c: &Connection, device_path: &str) -> Result<Vec<String>, Box<dyn Error>> {
    list_item(c, SERVICE_INTERFACE, device_path, "Device")
}

pub fn list_characteristics(
    c: &Connection,
    device_path: &str,
) -> Result<Vec<String>, Box<dyn Error>> {
    list_item(c, CHARACTERISTIC_INTERFACE, device_path, "Service")
}

pub fn list_descriptors(c: &Connection, device_path: &str) -> Result<Vec<String>, Box<dyn Error>> {
    list_item(c, DESCRIPTOR_INTERFACE, device_path, "Characteristic")
}

fn list_item(
    c: &Connection,
    item_interface: &str,
    item_path: &str,
    item_property: &str,
) -> Result<Vec<String>, Box<dyn Error>> {
    let mut v: Vec<String> = Vec::new();
    let objects: Vec<MessageItem> = get_managed_objects(&c)?;
    let z: &[MessageItem] = objects.get(0).unwrap().inner().unwrap();
    for y in z {
        let (path, interfaces) = y.inner().unwrap();
        let x: &[MessageItem] = interfaces.inner().unwrap();
        for interface in x {
            let (i, _) = interface.inner().unwrap();
            let name: &str = i.inner().unwrap();
            if name == item_interface {
                let objpath: &str = path.inner().unwrap();
                let prop = get_property(c, item_interface, objpath, item_property)?;
                let prop_path = prop.inner::<&str>().unwrap();
                if prop_path == item_path {
                    v.push(String::from(objpath));
                }
            }
        }
    }
    Ok(v)
}

pub fn get_property(
    c: &Connection,
    interface: &str,
    object_path: &str,
    prop: &str,
) -> Result<MessageItem, Box<dyn Error>> {
    let p = Props::new(&c, SERVICE_NAME, object_path, interface, 1000);
    Ok(p.get(prop)?.clone())
}

pub fn set_property<T>(
    c: &Connection,
    interface: &str,
    object_path: &str,
    prop: &str,
    value: T,
    timeout_ms: i32,
) -> Result<(), Box<dyn Error>>
where
    T: Into<MessageItem>,
{
    let p = Props::new(&c, SERVICE_NAME, object_path, interface, timeout_ms);
    Ok(p.set(prop, value.into())?)
}

pub fn call_method(
    c: &Connection,
    interface: &str,
    object_path: &str,
    method: &str,
    param: Option<&[MessageItem]>,
    timeout_ms: i32,
) -> Result<(), Box<dyn Error>> {
    let mut m = Message::new_method_call(
        SERVICE_NAME,
        object_path,
        interface,
        method
    )?;
    if let Some(p) = param {
        m.append_items(p);
    }
    c.send_with_reply_and_block(m, timeout_ms)?;
    Ok(())
}
