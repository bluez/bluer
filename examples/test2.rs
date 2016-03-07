extern crate blurz;

static BATTERY_SERVICE_UUID: &'static str = "0000180f-0000-1000-8000-00805f9b34fb";

use std::time::Duration;
use std::thread;

use blurz::bluetooth_adapter::BluetoothAdapter as BTAdapter;
use blurz::bluetooth_device::BluetoothDevice as BTDevice;
use blurz::bluetooth_gatt_service::BluetoothGATTService as BTGATTService;
use blurz::bluetooth_gatt_characteristic::BluetoothGATTCharacteristic as BTGATTCharacteristic;
use blurz::bluetooth_gatt_descriptor::BluetoothGATTDescriptor as BTGATTDescriptor;

fn error(error: String) {
    println!("{}", error);
    std::process::exit(1);
}

fn main() {
    let adapter: BTAdapter = match BTAdapter::init() {
        Ok(a) => a,
        Err(e) => return error(e),
    };
    match adapter.start_discovery() {
        Ok(_) => println!("Start discovery"),
        Err(e) => return error(e),
    }
    let devices = adapter.get_device_list();
    if devices.is_empty() {
        println!("No device found");
        match adapter.stop_discovery() {
            Ok(_) => println!("Stop discovery"),
            Err(e) => return error(e),
        }
    }
    println!("{} device(s) found", devices.len());
    let mut device: BTDevice = BTDevice::create_device("".to_string());
    'device_loop: for d in devices {
        device = BTDevice::create_device(d.clone());
        println!("{} {:?}", device.get_object_path(), device.get_alias());
        let uuids = match device.get_uuids() {
            Ok(u) => u,
            Err(e) => return error(e),
        };
        println!("{:?}", uuids);
        'uuid_loop: for uuid in uuids {
            if uuid == BATTERY_SERVICE_UUID {
                println!("{:?} has battery service!", device.get_alias());
                if !device.is_connected().unwrap() {
                    println!("connect device...");
                    match device.connect() {
                        Ok(_) => println!("connected!"),
                        Err(e) => return error(e),
                    }
                }
                break 'device_loop;
            }
        }
        println!("");
    }
    adapter.stop_discovery();
    println!("wait for it!!");
    // We need to wait a bit after calling connect to safely
    // get the gatt services
    thread::sleep(Duration::from_millis(1000));
    let services = match device.get_gatt_services() {
        Ok(s) => s,
        Err(e) => return error(e),
    };
    for service in services {
        let s = BTGATTService::new(service.clone());
        println!("{:?}", s);
        let characteristics = match s.get_characteristics() {
            Ok(c) => c,
            Err(e) => return error(e),
        };
        for characteristic in characteristics {
            let c = BTGATTCharacteristic::new(characteristic.clone());
            println!("{:?}", c);
            println!("Value: {:?} == {:?}", c.read_value(), c.get_value());
            println!("{:?}", c.write_value(vec![120u8]));
            println!("Value: {:?} == {:?}", c.read_value(), c.get_value());
            let descriptors = match c.get_descriptors() {
                Ok(d) => d,
                Err(e) => return error(e),
            };
            for descriptor in descriptors {
                let d = BTGATTDescriptor::new(descriptor.clone());
                println!("{:?}", d);
                println!("Value: {:?} == {:?}", d.read_value(), d.get_value());
                println!("{:?}", d.write_value(vec![0u8, 100u8]));
                println!("Value: {:?} == {:?}", d.read_value(), d.get_value());
            }

        }
    }
    device.disconnect();
}
