extern crate blurz;
extern crate dbus;

use dbus::arg::messageitem::MessageItem;
use std::error::Error;
use std::thread;
use std::time::Duration;

use blurz::bluetooth_adapter::BluetoothAdapter;
use blurz::bluetooth_device::BluetoothDevice;
use blurz::bluetooth_discovery_session::BluetoothDiscoverySession;
use blurz::bluetooth_le_advertising_data::BluetoothAdvertisingData;
use blurz::bluetooth_le_advertising_manager::BluetoothAdvertisingManager;
use blurz::bluetooth_session::BluetoothSession;

//const LEADVERTISING_MANAGER_INTERFACE: &str = "org.bluez.LEAdvertisingManager1";
const LEADVERTISING_DATA_INTERFACE: &str = "org.bluez.LEAdvertisement1";
const BATTERY_SERVICE_UUID: &str = "0000180f-0000-1000-8000-00805f9b34fb";
const COLOR_PICKER_SERVICE_UUID: &str = "00001812-0000-1000-8000-00805f9b34fb";

fn main() -> Result<(), Box<dyn Error>> {
    let bt_session = BluetoothSession::create_session(None)?;
    let adapter = BluetoothAdapter::init(&bt_session)?;
    let session = BluetoothDiscoverySession::create_session(&bt_session, &adapter.get_id())?;
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
    let mut device = BluetoothDevice::new(&bt_session, "");
    'device_loop: for d in devices {
        device = BluetoothDevice::new(&bt_session, &d);
        println!("{} {:?}", device.get_id(), device.get_alias());
        let uuids = device.get_uuids()?;
        println!("{:?}", uuids);
        '_uuid_loop: for uuid in uuids {
            if (uuid == COLOR_PICKER_SERVICE_UUID) || (uuid == BATTERY_SERVICE_UUID) {
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
                    println!("couldn't connect");
                }
            }
        }
        println!();
    }
    //adapter.stop_discovery().ok();
    if !device.is_connected()? {
        return Err(Box::from("No connectable device found"));
    }

    let addata = BluetoothAdvertisingData::new(&bt_session, LEADVERTISING_DATA_INTERFACE);

    let manager = BluetoothAdvertisingManager::create_adv_manager()?;
    println!("{:?} : {:?}", manager, manager.get_conn());
    manager.register_advertisement([
        addata.get_object_path().into(),
        MessageItem::new_dict(vec![(0u8.into(), 0u8.into())]).unwrap(),
    ])?;

    Ok(())
}
