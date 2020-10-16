extern crate blurz;
extern crate dbus;

use std::error::Error;
use std::time::Duration;
use std::thread;
use dbus::arg::messageitem::MessageItem;

use blurz::bluetooth_adapter::BluetoothAdapter as Adapter;
use blurz::bluetooth_device::BluetoothDevice as Device;
//use blurz::bluetooth_gatt_service::BluetoothGATTService as Service;
//use blurz::bluetooth_gatt_characteristic::BluetoothGATTCharacteristic as Characteristic;
//use blurz::bluetooth_gatt_descriptor::BluetoothGATTDescriptor as Descriptor;
use blurz::bluetooth_discovery_session::BluetoothDiscoverySession as DiscoverySession;
use blurz::bluetooth_session::BluetoothSession as Session;
use blurz::bluetooth_le_advertising_data::BluetoothAdvertisingData as AdData;
use blurz::bluetooth_le_advertising_manager::BluetoothAdvertisingManager as Manager;

//static LEADVERTISING_MANAGER_INTERFACE: &'static str = "org.bluez.LEAdvertisingManager1";
static LEADVERTISING_DATA_INTERFACE: &'static str = "org.bluez.LEAdvertisement1";
static BATTERY_SERVICE_UUID: &'static str = "0000180f-0000-1000-8000-00805f9b34fb";
static COLOR_PICKER_SERVICE_UUID: &'static str = "00001812-0000-1000-8000-00805f9b34fb";

fn test_advertising() -> Result<(), Box<dyn Error>>{
    let bt_session = &Session::create_session(None)?;
    let adapter: Adapter = Adapter::init(bt_session)?;
    let session = DiscoverySession::create_session(
        &bt_session,
        adapter.get_id()
    )?;
    session.start_discovery()?;
    for _ in 0..5 {
        let devices = adapter.get_device_list()?;
        if !devices.is_empty() {
            break;
        }
        thread::sleep(Duration::from_millis(1000));
    }
    session.stop_discovery()?;
    let devices = adapter.get_device_list()?;
    if devices.is_empty() {
        return Err(Box::from("No device found"));
    }
    println!("{} device(s) found", devices.len());
    let mut device: Device = Device::new(&bt_session, "".to_string());
    'device_loop: for d in devices {
        device = Device::new(&bt_session, d.clone());
        println!("{} {:?}", device.get_id(), device.get_alias());
        let uuids = device.get_uuids()?;
        println!("{:?}", uuids);
        '_uuid_loop:
        for uuid in uuids {
            if uuid == COLOR_PICKER_SERVICE_UUID ||
               uuid == BATTERY_SERVICE_UUID {
                println!("{:?} has a service!", device.get_alias());
                println!("connect device...");
                device.connect(5000).ok();
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
        println!("");
    }
    adapter.stop_discovery().ok();
    if !device.is_connected()? {
        return Err(Box::from("No connectable device found"));
    }

    let addata = AdData::new(&bt_session, LEADVERTISING_DATA_INTERFACE.to_string());
    
    let manager = Manager::create_adv_manager()?;
    println!("{:?} : {:?}", manager, manager.get_conn());
    manager.register_advertisement([addata.get_object_path().into(), MessageItem::new_dict(vec![(0u8.into(), 0u8.into())]).unwrap()])?;

    Ok(())
}


fn main() {
    match test_advertising() {
         Ok(_) => (),
         Err(e) => println!("{:?}", e),
    }
}
