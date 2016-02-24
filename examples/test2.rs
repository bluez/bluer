extern crate blurz;

static BATTERY_SERVICE_UUID: &'static str = "0000180f-0000-1000-8000-00805f9b34fb";

use std::time::Duration;
use std::thread;

use blurz::bluetooth_adapter::BluetoothAdapter as BTAdapter;
use blurz::bluetooth_device::BluetoothDevice as BTDevice;
use blurz::bluetooth_gatt_service::BluetoothGATTService as BTGATTService;
use blurz::bluetooth_gatt_characteristic::BluetoothGATTCharacteristic as BTGATTCharacteristic;

fn main() {
    let adapter: BTAdapter = BTAdapter::init().unwrap();
    adapter.start_discovery();
    let devices = adapter.get_devices();
    let mut device: BTDevice = BTDevice::create_device("".to_string());
    'device_loop: for d in devices {
    	device = BTDevice::create_device(d.clone());
    	println!("{:?}", device);
    	let uuids = device.get_uuids();
    	println!("{:?}", uuids);
    	'uuid_loop: for uuid in uuids {
    		if uuid == BATTERY_SERVICE_UUID {
    			println!("{:?} has battery service!", device.get_alias());
    			println!("connect device...");
    			if device.is_connected() {
    				device.disconnect();
    			}
    			device.connect();
    			println!("connected!!");
    			break 'device_loop;
    		}
    	}
    	println!("");
    }
    adapter.stop_discovery();
    println!("wait for it!!");
    thread::sleep(Duration::from_millis(3000));
    let services = device.get_gatt_services();
    for service in services {
        let s = BTGATTService::new(service.clone());
    	println!("{:?}", s);
        let cs = s.get_characteristics();
        for c in cs {
            let characteristic = BTGATTCharacteristic::new(c.clone());
            println!("{:?}", characteristic);
            println!("Value: {:?}", characteristic.read_value());
        }
    }
}
