extern crate blurz;

static BATTERY_SERVICE_UUID: &'static str = "0000180f-0000-1000-8000-00805f9b34fb";

use std::error::Error;
use std::time::Duration;
use std::thread;

use blurz::bluetooth_adapter::BluetoothAdapter as Adapter;
use blurz::bluetooth_device::BluetoothDevice as Device;
use blurz::bluetooth_gatt_service::BluetoothGATTService as Service;
use blurz::bluetooth_gatt_characteristic::BluetoothGATTCharacteristic as Characteristic;
use blurz::bluetooth_gatt_descriptor::BluetoothGATTDescriptor as Descriptor;

fn test2() -> Result<(), Box<Error>> {
    let adapter: Adapter = try!(Adapter::init());
    try!(adapter.start_discovery());
    let devices = try!(adapter.get_device_list());
    if devices.is_empty() {
        adapter.stop_discovery().ok();
        return Err(Box::from("No device found"));
    }
    println!("{} device(s) found", devices.len());
    let mut device: Device = Device::create_device("".to_string());
    'device_loop: for d in devices {
        device = Device::create_device(d.clone());
        println!("{} {:?}", device.get_object_path(), device.get_alias());
        let uuids = try!(device.get_uuids());
        println!("{:?}", uuids);
        'uuid_loop: for uuid in uuids {
            if uuid == BATTERY_SERVICE_UUID {
                println!("{:?} has battery service!", device.get_alias());
                println!("connect device...");
                device.connect().ok();
                if try!(device.is_connected()) {
                    println!("checking gatt...");
                    // We need to wait a bit after calling connect to safely
                    // get the gatt services
                    thread::sleep(Duration::from_millis(5000));
                    match device.get_gatt_services() {
                        Ok(_) => break 'device_loop,
                        Err(e) => println!("{:?}", e),
                    }
                } else {
                    println!("could not connect");
                }
            }
        }
        println!("");
    }
    adapter.stop_discovery().ok();
    if !try!(device.is_connected()) {
        return Err(Box::from("No connectable device found"));
    }
    let services = try!(device.get_gatt_services());
    for service in services {
        let s = Service::new(service.clone());
        println!("{:?}", s);
        let characteristics = try!(s.get_gatt_characteristics());
        for characteristic in characteristics {
            let c = Characteristic::new(characteristic.clone());
            println!("{:?}", c);
            println!("Value: {:?}", c.read_value());
            let descriptors = try!(c.get_gatt_descriptors());
            for descriptor in descriptors {
                let d = Descriptor::new(descriptor.clone());
                println!("{:?}", d);
                println!("Value: {:?}", d.read_value());
            }

        }
    }
    try!(device.disconnect());
    Ok(())
}

fn main() {
    match test2() {
         Ok(_) => (),
         Err(e) => println!("{:?}", e),
     }
}
