extern crate blurz;

use std::error::Error;

use blurz::bluetooth_event::BluetoothEvent;
use blurz::bluetooth_session::BluetoothSession as Session;

fn test5() -> Result<(), Box<Error>> {
    let session = &Session::create_session(Some("/org/bluez/hci0")).unwrap();
    loop {
        for event in session.incoming(1000).map(BluetoothEvent::from) {
            println!("{:?}", event);
        }
    }
}

fn main() {
    match test5() {
        Ok(_) => (),
        Err(e) => println!("{:?}", e),
    }
}
