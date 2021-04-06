use dbus::arg::messageitem::{MessageItem, Props};
use dbus::ffidisp::{BusType, Connection};
use dbus::{Error as DbusError, Message};
use std::error::Error;

static ADAPTER_INTERFACE: &str = "org.bluez.Adapter1";
static DEVICE_INTERFACE: &str = "org.bluez.Device1";
static SERVICE_INTERFACE: &str = "org.bluez.GattService1";
static CHARACTERISTIC_INTERFACE: &str = "org.bluez.GattCharacteristic1";
static DESCRIPTOR_INTERFACE: &str = "org.bluez.GattDescriptor1";
pub static SERVICE_NAME: &str = "org.bluez";
static LEADVERTISING_DATA_INTERFACE: &str = "org.bluez.LEAdvertisement1";
static LEADVERTISING_MANAGER_INTERFACE: &str = "org.bluez.LEAdvertisingManager1";

#[macro_export]
macro_rules! ok_or_str {
    ($e: expr) => {
        $e.map_err(|e| Box::<dyn Error>::from(format!("{:?}", e)))
    }
}

#[macro_export]
macro_rules! or_else_str {
    ($e: expr, $message: literal) => {
        $e.ok_or_else(|| DbusError::new_custom("missing", $message))
    }
}


fn get_managed_objects(c: &Connection) -> Result<Vec<MessageItem>, Box<dyn Error>> {
    let m = Message::new_method_call(
        SERVICE_NAME,
        "/",
        "org.freedesktop.DBus.ObjectManager",
        "GetManagedObjects",
    )?;
    let r = c.send_with_reply_and_block(m, 1000)?;
    Ok(r.get_items())
}

pub fn get_adapters(c: &Connection) -> Result<Vec<String>, Box<dyn Error>> {
    let mut adapters = Vec::new();
    let objects = get_managed_objects(&c)?;
    let z: &[(MessageItem, MessageItem)] = ok_or_str!(
        or_else_str!(objects.get(0), "get_adapters couldn't get managed objects")?.inner())?;
    for (path, interfaces) in z {
        let x: &[(MessageItem, MessageItem)] = ok_or_str!(interfaces.inner())?;
        for (i, _) in x {
            let name: &str = ok_or_str!(i.inner())?;
            if name == ADAPTER_INTERFACE {
                let p: &str = ok_or_str!(path.inner())?;
                adapters.push(String::from(p));
            }
        }
    }
    Ok(adapters)
}

pub fn get_ad_man() -> Result<Vec<String>, Box<dyn Error>> {
    let mut managers = Vec::new();
    let c = Connection::get_private(BusType::System)?;
    let objects = get_managed_objects(&c)?;
    let z: &[(MessageItem, MessageItem)] = ok_or_str!(or_else_str!(objects.get(0), "get_ad_mancouldn't get managed objects")?.inner())?;
    for (path, interfaces) in z {
        let x: &[(MessageItem, MessageItem)] = ok_or_str!(interfaces.inner())?;
        for (i, _) in x {
            let name: &str = ok_or_str!(i.inner())?;
            if name == LEADVERTISING_MANAGER_INTERFACE {
                let p: &str = ok_or_str!(path.inner())?;
                managers.push(String::from(p));
            }
        }
    }
    Ok(managers)
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

pub fn list_addata_1(c: &Connection, adapter_path: &str) -> Result<Vec<String>, Box<dyn Error>> {
    list_item(
        c,
        LEADVERTISING_DATA_INTERFACE,
        adapter_path,
        "Advertisement",
    )
}

pub fn list_addata_2(c: &Connection, device_path: &str) -> Result<Vec<String>, Box<dyn Error>> {
    list_item(
        c,
        LEADVERTISING_DATA_INTERFACE,
        device_path,
        "Advertisement",
    )
}

fn list_item(
    c: &Connection,
    item_interface: &str,
    item_path: &str,
    item_property: &str,
) -> Result<Vec<String>, Box<dyn Error>> {
    let mut v: Vec<String> = Vec::new();
    let objects: Vec<MessageItem> = get_managed_objects(&c)?;
    let z: &[(MessageItem, MessageItem)] = ok_or_str!(or_else_str!(objects.get(0), "get_ad_mancouldn't get managed objects")?.inner())?;
    for (path, interfaces) in z {
        let x: &[(MessageItem, MessageItem)] = ok_or_str!(interfaces.inner())?;
        for (i, _) in x {
            let name: &str = ok_or_str!(i.inner())?;
            if name == item_interface {
                let objpath: &str = ok_or_str!(path.inner())?;
                let prop = get_property(c, item_interface, objpath, item_property)?;
                let prop_path = ok_or_str!(prop.inner::<&str>())?;
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
    Ok(p.get(prop)?)
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
) -> Result<Message, Box<dyn Error>> {
    let mut m = Message::new_method_call(SERVICE_NAME, object_path, interface, method)?;
    if let Some(p) = param {
        m.append_items(p);
    }
    Ok(c.send_with_reply_and_block(m, timeout_ms)?)
}
