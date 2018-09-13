extern crate blurz;

use std::error::Error;
use std::fs;
use std::fs::File;
use std::io::Write;
use std::path::Path;

use blurz::bluetooth_adapter::BluetoothAdapter as Adapter;
use blurz::bluetooth_device::BluetoothDevice as Device;
use blurz::bluetooth_obex::{
    BluetoothOBEXSession as OBEXSession, BluetoothOBEXTransfer as OBEXTransfer,
};
use blurz::bluetooth_session::BluetoothSession as Session;

fn test_obex_file_transfer() -> Result<(), Box<Error>> {
    let session = &Session::create_session(None)?;
    let adapter: Adapter = Adapter::init(session)?;
    let devices: Vec<String> = adapter.get_device_list()?;

    let filtered_devices = devices
        .iter()
        .filter(|&device_id| {
            let device = Device::new(session, device_id.to_string());
            device.is_ready_to_receive().unwrap()
        }).cloned()
        .collect::<Vec<String>>();

    let device_id: &str = &filtered_devices[0];
    let device = Device::new(session, device_id.to_string());

    let session = OBEXSession::new(session, &device)?;

    let mut empty_file = File::create("./test.png")?;
    empty_file.write_all(b"1111")?;

    let file_path = Path::new("./test.png").canonicalize()?;
    let file_str = file_path.to_str().unwrap();
    let transfer = OBEXTransfer::send_file(&session, file_str)?;
    transfer.wait_until_transfer_completed()?;

    session.remove_session()?;
    fs::remove_file(&file_path)?;
    Ok(())
}

fn main() {
    match test_obex_file_transfer() {
        Ok(_) => (),
        Err(e) => println!("{:?}", e),
    }
}
