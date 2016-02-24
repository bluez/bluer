use dbus::{Connection, BusType, Props, Message, MessageItem};
use bluetooth_utils;
use dbus::arg::Array;

static SERVICE_NAME: &'static str = "org.bluez";
static GATT_CHARACTERISTIC_INTERFACE: &'static str = "org.bluez.GattCharacteristic1";

#[derive(Clone, Debug)]
pub struct BluetoothGATTCharacteristic {
    object_path: String,
}

impl BluetoothGATTCharacteristic {
    pub fn new(object_path: String)
           -> BluetoothGATTCharacteristic {
        BluetoothGATTCharacteristic {
            object_path: object_path
        }
    }

    pub fn read_value(&self) -> u8 {
        let c = Connection::get_private(BusType::System).unwrap();
        let m = Message::new_method_call(SERVICE_NAME, &self.object_path, GATT_CHARACTERISTIC_INTERFACE, "ReadValue").unwrap();
        let r = c.send_with_reply_and_block(m, 1000).unwrap();
        let items = r.get_items();
        //println!("{:?}", items);
        let mut value: u8 = 0;
        for item in items {
        	//println!("{:?}", item);
	        let z: &[MessageItem] = item.inner().unwrap();

        	for i in z {
        		value = i.inner::<u8>().unwrap();
        		//println!("{:?}", value);
        	}
        }
        //println!("{:?}", arr);
        value
    }
}
