extern crate blurz;

use std::error::Error;
use std::thread;
use std::time::Duration;

use blurz::bluetooth_adapter::BluetoothAdapter as Adapter;
use blurz::bluetooth_device::BluetoothDevice as Device;
use blurz::bluetooth_discovery_session::BluetoothDiscoverySession as DiscoverySession;
use blurz::bluetooth_session::BluetoothSession as Session;

fn test3() -> Result<(), Box<Error>> {
    let bt_session = &Session::create_session(None)?;
    let adapter: Adapter = try!(Adapter::init(bt_session));
    try!(adapter.set_powered(true));
    loop {
        let session = try!(DiscoverySession::create_session(
            &bt_session,
            adapter.get_id()
        ));
        thread::sleep(Duration::from_millis(200));
        try!(session.start_discovery());
        thread::sleep(Duration::from_millis(800));
        let devices = try!(adapter.get_device_list());

        println!("{} device(s) found", devices.len());
        'device_loop: for d in devices {
            let device = Device::new(bt_session, d.clone());
            println!(
                "{} {:?} {:?}",
                device.get_id(),
                device.get_address(),
                device.get_rssi()
            );
            try!(adapter.remove_device(device.get_id()));
        }
        try!(session.stop_discovery());
    }
}

fn main() {
    match test3() {
        Ok(_) => (),
        Err(e) => println!("{:?}", e),
    }
}
