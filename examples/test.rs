extern crate blurz;

use blurz::bluetooth_adapter::BluetoothAdapter as BTAdapter;
use blurz::bluetooth_device::BluetoothDevice as BTDevice;

fn main() {
    let adapter: BTAdapter = BTAdapter::init().unwrap();
    let device: BTDevice = adapter.get_first_device().unwrap();
    println!("{:?}", device);
}
