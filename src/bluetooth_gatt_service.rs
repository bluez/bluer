use dbus::{Connection, BusType, Props, MessageItem};

static SERVICE_NAME: &'static str = "org.bluez";
static GATT_SERVICE_INTERFACE: &'static str = "org.bluez.GattService1";

#[derive(Clone, Debug)]
pub struct BluetoothGATTService {
    object_path: String,
}

impl BluetoothGATTService {
    pub fn new(object_path: String)
           -> BluetoothGATTService {
        BluetoothGATTService {
            object_path: object_path
        }
    }

    pub fn get_characteristics(&self) -> Vec<String> {
        let c = Connection::get_private(BusType::System).unwrap();
        let d = Props::new(&c, SERVICE_NAME, &self.object_path, GATT_SERVICE_INTERFACE, 10000);
        let characteristics = d.get("Characteristics").unwrap();
        let z: &[MessageItem] = characteristics.inner().unwrap();
        let mut v: Vec<String> = Vec::new();
        for y in z {
            v.push(String::from(y.inner::<&str>().unwrap()));
        }
        v
    }
}
