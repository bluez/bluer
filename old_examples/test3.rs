extern crate blurz;

use std::{error::Error, thread, time::Duration};

use blurz::{
    bluetooth_adapter::BluetoothAdapter as Adapter, bluetooth_device::BluetoothDevice as Device,
    bluetooth_discovery_session::BluetoothDiscoverySession as DiscoverySession, bluetooth_session::Session,
};

fn test3() -> Result<(), Box<dyn Error>> {
    let bt_session = &Session::create_session(None)?;
    let adapter: Adapter = Adapter::init(bt_session)?;
    adapter.set_powered(true)?;
    loop {
        let session = DiscoverySession::create_session(&bt_session, &adapter.get_id())?;
        thread::sleep(Duration::from_millis(200));
        session.start_discovery()?;
        thread::sleep(Duration::from_millis(800));
        let devices = adapter.get_device_list()?;

        println!("{} device(s) found", devices.len());
        '_device_loop: for d in devices {
            let device = Device::new(bt_session, &d);
            println!("{} {:?} {:?}", device.get_id(), device.get_address(), device.get_rssi());
            adapter.remove_device(&device.get_id())?;
        }
        session.stop_discovery()?;
    }
}

fn main() {
    match test3() {
        Ok(_) => (),
        Err(e) => println!("{:?}", e),
    }
}
