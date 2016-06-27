extern crate blurz;
extern crate dbus;

use std::error::Error;
use std::time::Duration;
use std::thread;
use dbus::{Connection, BusType, Message, MessageItem, Props};

use blurz::bluetooth_adapter::BluetoothAdapter as Adapter;
use blurz::bluetooth_device::BluetoothDevice as Device;
use blurz::bluetooth_gatt_service::BluetoothGATTService as Service;
use blurz::bluetooth_gatt_characteristic::BluetoothGATTCharacteristic as Characteristic;
use blurz::bluetooth_gatt_descriptor::BluetoothGATTDescriptor as Descriptor;
use blurz::bluetooth_discovery_session::BluetoothDiscoverySession as DiscoverySession;
use blurz::bluetooth_le_advertising_data::BluetoothAdvertisingData as AdData;
use blurz::bluetooth_le_advertising_manager::BluetoothAdvertisingManager as Manager;

static LEADVERTISING_MANAGER_INTERFACE: &'static str = "org.bluez.LEAdvertisingManager1";
static LEADVERTISING_DATA_INTERFACE: &'static str = "org.bluez.LEAdvertisement1";
static BATTERY_SERVICE_UUID: &'static str = "0000180f-0000-1000-8000-00805f9b34fb";
static COLOR_PICKER_SERVICE_UUID: &'static str = "00001812-0000-1000-8000-00805f9b34fb";

fn test_advertising() -> Result<(), Box<Error>>{
	let adapter: Adapter = try!(Adapter::init());
    let session = try!(DiscoverySession::create_session(adapter.get_object_path()));
    try!(session.start_discovery());
    //let mut devices = vec!();
    for _ in 0..5 {
        let devices = try!(adapter.get_device_list());
        if !devices.is_empty() {
            break;
        }
        thread::sleep(Duration::from_millis(1000));
    }
    try!(session.stop_discovery());
    let devices = try!(adapter.get_device_list());
    if devices.is_empty() {
        return Err(Box::from("No device found"));
    }
    println!("{} device(s) found", devices.len());
    let mut device: Device = Device::new("".to_string());
    'device_loop: for d in devices {
        device = Device::new(d.clone());
        println!("{} {:?}", device.get_object_path(), device.get_alias());
        let uuids = try!(device.get_uuids());
        println!("{:?}", uuids);
        'uuid_loop: for uuid in uuids {
            if uuid == COLOR_PICKER_SERVICE_UUID ||
               uuid == BATTERY_SERVICE_UUID {
                println!("{:?} has a service!", device.get_alias());
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

    let addata = AdData::new(LEADVERTISING_DATA_INTERFACE.to_string());
    
    let manager = try!(Manager::create_adv_manager());
    println!("{:?} : {:?}", manager, manager.get_conn());
    try!(manager.register_advertisement([addata.get_object_path().into(), MessageItem::DictEntry(Box::new(MessageItem::Byte(0)), Box::new(MessageItem::Byte(0)))]));

    Ok(())
}


fn main() {
    match test_advertising() {
         Ok(_) => (),
         Err(e) => println!("{:?}", e),
    }
}