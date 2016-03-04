extern crate blurz;

use blurz::bluetooth_adapter::BluetoothAdapter as BTAdapter;
use blurz::bluetooth_device::BluetoothDevice as BTDevice;

fn main() {

    let adapter: BTAdapter = match BTAdapter::init() {
        Ok(a) => a,
        Err(e) => {
            println!("{}", e);
            std::process::exit(1);
        }
    };
    let device: BTDevice = match adapter.get_first_device() {
        Ok(d) => d,
        Err(e) => {
            println!("{}", e);
            std::process::exit(1);
        }
    };
    println!("{:?}", device);
}
