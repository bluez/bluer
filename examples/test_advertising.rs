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
    let manager: Manager = try!(Manager::init());
    println!("{:?}", manager);
    try!(manager.register_advertisement(Some([LEADVERTISING_DATA_INTERFACE.into(), .into()])));
    /*let addata1: AdData = try!(adapter.get_addata());
    println!("{:?}", addata1);
    let device: Device = Device::new(devices[4].clone());
    println!("{:?}", device);
    let addata2: AdData = try!(device.get_addata());
    println!("{:?}", addata2.include_tx_power());*/




    Ok(())
}


fn main() {
    match test_advertising() {
         Ok(_) => (),
         Err(e) => println!("{:?}", e),
    }
}