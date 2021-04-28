extern crate blurz;

use std::{error::Error, fs, fs::File, io::Write, path::Path};

use blurz::{
    bluetooth_adapter::BluetoothAdapter as Adapter,
    bluetooth_device::BluetoothDevice as Device,
    bluetooth_obex::{BluetoothOBEXSession as OBEXSession, BluetoothOBEXTransfer as OBEXTransfer},
    bluetooth_session::Session,
};

fn test_obex_file_transfer() -> Result<(), Box<dyn Error>> {
    let session = &Session::create_session(None)?;
    let adapter: Adapter = Adapter::init(session)?;
    let devices: Vec<String> = adapter.get_device_list()?;

    let filtered_devices = devices
        .iter()
        .filter(|&device_id| {
            let device = Device::new(session, device_id);
            device.is_ready_to_receive().unwrap()
        })
        .cloned()
        .collect::<Vec<String>>();

    let device_id: &str = &filtered_devices[0];
    let device = Device::new(session, device_id);

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
