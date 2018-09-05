extern crate blurz;

use std::error::Error;

use blurz::bluetooth_session::BluetoothSession as Session;
use blurz::bluetooth_adapter::BluetoothAdapter as Adapter;
use blurz::bluetooth_device::BluetoothDevice as Device;

fn test() -> Result<(), Box<Error>> {
    let session = &Session::create_session().unwrap();
    let adapter: Adapter = try!(Adapter::init(session));
    let device: Device = try!(adapter.get_first_device());
    println!("{:?}", device);
    Ok(())
}

fn main() {
    match test() {
        Ok(_) => (),
        Err(e) => println!("{:?}", e),
    }
}
