use dbus::{Connection, BusType, Message, MessageItem, Props};

static ADAPTER_INTERFACE: &'static str = "org.bluez.Adapter1";
static DEVICE_INTERFACE: &'static str = "org.bluez.Device1";
static SERVICE_NAME: &'static str = "org.bluez";

pub fn get_managed_objects(c: &Connection) ->  Vec<MessageItem> {
    let m = Message::new_method_call("org.bluez", "/", "org.freedesktop.DBus.ObjectManager", "GetManagedObjects").unwrap();
    let r = c.send_with_reply_and_block(m, 2000).unwrap();
    r.get_items()
}

pub fn get_adapters() -> Vec<String> {
    let mut adapters: Vec<String> = Vec::new();
    let c = Connection::get_private(BusType::System).unwrap();
    let objects: Vec<MessageItem> = get_managed_objects(&c);
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
    adapters
}

pub fn list_devices(adapter_path: &String) -> Vec<String>{
    let mut v: Vec<String> = Vec::new();
    let c = Connection::get_private(BusType::System).unwrap();
    let objects: Vec<MessageItem> = get_managed_objects(&c);
    let z: &[MessageItem] = objects.get(0).unwrap().inner().unwrap();
    for y in z {
        let (path, interfaces) = y.inner().unwrap();
        let x: &[MessageItem] = interfaces.inner().unwrap();
        for interface in x {
            let (i,_) = interface.inner().unwrap();
            let name: &str = i.inner().unwrap();
            if name == DEVICE_INTERFACE {
                let objpath: &str = path.inner().unwrap();
                let device = Props::new(&c, SERVICE_NAME, String::from(objpath), DEVICE_INTERFACE, 10000);
                let adapter = &String::from(device.get("Adapter").unwrap().inner::<&str>().unwrap());
                if adapter == adapter_path {
                    v.push(String::from(objpath));
                }
            }
        }
    }
    v
}

pub fn get_property(interface: &str, object_path: &str, prop: &str) -> Result<MessageItem, String> {
    let c = match Connection::get_private(BusType::System) {
        Ok(conn) => conn,
        Err(_) => return Err(String::from("Error! Connecting to dbus."))
    };
    let p = Props::new(&c, SERVICE_NAME, object_path, interface, 1000);
    match p.get(prop) {
        Ok(p) => Ok(p.clone()),
        Err(_) => Err(String::from("Error! Getting property: ")+prop),
    }
}

pub fn set_property<T>(interface: &str, object_path: &str, prop: &str, value: T) -> Result<(), String>
where T: Into<MessageItem> {
    let c = match Connection::get_private(BusType::System) {
        Ok(conn) => conn,
        Err(_) => return Err(String::from("Error! Connecting to dbus."))
    };
    let p = Props::new(&c, SERVICE_NAME, object_path, interface, 1000);
    match p.set(prop, value.into()) {
        Ok(_) => Ok(()),
        Err(_) => Err(String::from("Error! Setting property: ")+prop),
    }
}
