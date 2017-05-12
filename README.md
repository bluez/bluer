Bluetooth lib for Rust using blueZ/dbus
=======================================

Current state: Experimental
Required bluez version: 5.44

Examples
========
This example show how to get the first available bluetooth device.
``` rust
let adapter: BluetoothAdapter = BluetoothAdapter::init().unwrap();
let device: BluetoothDevice = adapter.get_first_device().unwrap();
println!("{:?}", device);
```
