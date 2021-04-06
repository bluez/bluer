extern crate blurz;

const BATTERY_SERVICE_UUID: &str = "0000180f-0000-1000-8000-00805f9b34fb";
const COLOR_PICKER_SERVICE_UUID: &str = "00001812-0000-1000-8000-00805f9b34fb";

use std::error::Error;
use std::thread;
use std::time::Duration;

use blurz::bluetooth_adapter::BluetoothAdapter as Adapter;
use blurz::bluetooth_device::BluetoothDevice as Device;
use blurz::bluetooth_discovery_session::BluetoothDiscoverySession as DiscoverySession;
use blurz::bluetooth_gatt_characteristic::BluetoothGATTCharacteristic as Characteristic;
use blurz::bluetooth_gatt_descriptor::BluetoothGATTDescriptor as Descriptor;
use blurz::bluetooth_gatt_service::BluetoothGATTService as Service;
use blurz::bluetooth_session::BluetoothSession as Session;

fn test2() -> Result<(), Box<dyn Error>> {
    let bt_session = &Session::create_session(None)?;
    let adapter: Adapter = Adapter::init(bt_session)?;
    let session = DiscoverySession::create_session(&bt_session, &adapter.get_id())?;
    session.start_discovery()?;
    let mut devices = vec![];
    for _ in 0..5 {
        devices = adapter.get_device_list()?;
        if !devices.is_empty() {
            break;
        }
        thread::sleep(Duration::from_millis(1000));
    }
    session.stop_discovery()?;
    if devices.is_empty() {
        return Err(Box::from("No device found"));
    }

    println!("{} device(s) found", devices.len());
    let mut device: Device = Device::new(&bt_session, "");
    'device_loop: for d in devices {
        device = Device::new(bt_session, &d);
        println!("{} {:?}", device.get_id(), device.get_alias());
        let uuids = device.get_uuids()?;
        println!("{:?}", uuids);
        '_uuid_loop: for uuid in uuids {
            if uuid == COLOR_PICKER_SERVICE_UUID || uuid == BATTERY_SERVICE_UUID {
                println!("{:?} has a service!", device.get_alias());
                println!("connect device...");
                device.connect(10000).ok();
                if device.is_connected()? {
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
        println!();
    }
    adapter.stop_discovery().ok();
    if !device.is_connected()? {
        return Err(Box::from("No connectable device found"));
    }
    let services = device.get_gatt_services()?;
    for service in services {
        let s = Service::new(bt_session, &service);
        println!("{:?}", s);
        let characteristics = s.get_gatt_characteristics()?;
        for characteristic in characteristics {
            let c = Characteristic::new(bt_session, &characteristic);
            println!("{:?}", c);
            println!("Value: {:?}", c.read_value(None));
            let descriptors = c.get_gatt_descriptors()?;
            for descriptor in descriptors {
                let d = Descriptor::new(bt_session, &descriptor);
                println!("{:?}", d);
                println!("Value: {:?}", d.read_value(None));
            }
        }
    }
    device.disconnect()?;
    Ok(())
}

fn main() {
    match test2() {
        Ok(_) => (),
        Err(e) => println!("{:?}", e),
    }
}
