extern crate blurz;

use std::error::Error;
use std::thread;
use std::time::Duration;

use blurz::bluetooth_adapter::BluetoothAdapter as Adapter;
use blurz::bluetooth_device::BluetoothDevice as Device;
use blurz::bluetooth_discovery_session::BluetoothDiscoverySession as DiscoverySession;
use blurz::bluetooth_session::BluetoothSession as Session;
use blurz::BluetoothGATTService;
use blurz::BluetoothGATTCharacteristic;


fn test6() -> Result<(), Box<Error>> {
    let bt_session = &Session::create_session(None)?;
    let adapter: Adapter = Adapter::init(bt_session)?;
    adapter.set_powered(true)?;

    let session = DiscoverySession::create_session(
        &bt_session,
        adapter.get_id()
    )?;
    thread::sleep(Duration::from_millis(200));
    session.start_discovery()?;
    thread::sleep(Duration::from_millis(800));
    let devices = adapter.get_device_list()?;

    println!("{} device(s) found", devices.len());
    println!();

    'device_loop: for d in devices {
        let device = Device::new(bt_session, d.clone());

        println!(
            "Device: Id: {} Address: {:?} Rssi: {:?} Name: {:?}",
            device.get_id(),
            device.get_address(),
            device.get_rssi(),
            device.get_name()
        );

        if let Err(e) = device.pair() {
            println!("  Error on pairing: {:?}", e);
        }

        println!("  Is paired: {:?}", device.is_paired());

        //device.connect(5000);

        println!("  Is connected: {:?}", device.is_connected());

        match device.is_ready_to_receive() {
            Some(v) => println!("  Is ready to receive: {:?}", v),
            None => println!("  Error is_ready_to_receive()")
        }

        let all_gatt_services = device.get_gatt_services();

        match all_gatt_services {
            Ok(gatt_services) => {
                for service in gatt_services {
                    let gatt_service = BluetoothGATTService::new(bt_session, service);

                    println!("  Gatt service Id: {} UUID: {:?} Device : {:?} Is primary: {:?}",
                             gatt_service.get_id(),
                             gatt_service.get_uuid(),
                             gatt_service.get_device(),
                             gatt_service.is_primary());

                    match gatt_service.get_gatt_characteristics() {
                        Ok(ref gat_chars) => {
                            for characteristics in gat_chars {
                                let gatt_char = BluetoothGATTCharacteristic::new(bt_session, characteristics.to_owned());

                                println!("    Characteristic Name: {} UUID: {:?} Flags: {:?}",
                                         characteristics, gatt_char.get_uuid(),
                                         gatt_char.get_flags());
                            }
                        },
                        Err(e) => println!("    Error get_gatt_characteristics(): {:?}", e)
                    }
                }
            },
            Err(e) => println!("{:?}", e)
        }

        if let Err(e) = device.disconnect() {
            println!("  Error on disconnect: {:?}", e);
        }

        adapter.remove_device(device.get_id())?;
    }
    session.stop_discovery()?;

    Ok(())

}

fn main() {
    match test6() {
        Ok(_) => (),
        Err(e) => println!("{:?}", e),
    }
}
