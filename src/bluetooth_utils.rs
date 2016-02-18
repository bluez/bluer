use dbus::{Connection, BusType, Message, MessageItem, Props};

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
            if name == "org.bluez.Adapter1" {
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
            if name == "org.bluez.Device1" {
                let objpath: &str = path.inner().unwrap();
                let device = Props::new(&c, "org.bluez", String::from(objpath), "org.bluez.Device1", 10000);
                let adapter = &String::from(device.get("Adapter").unwrap().inner::<&str>().unwrap());
                if adapter == adapter_path {
                    v.push(String::from(objpath));
                }
            }
        }
    }
    v
}
