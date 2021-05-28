BLEZ - Asynchronous Bluetooth Low Energy for Rust using BlueZ
=============================================================

This library provides an asynchronous, fully featured interface to the [Bluetooth Low Energy (BLE)](https://en.wikipedia.org/wiki/Bluetooth_Low_Energy)
APIs of the [official Linux Bluetooth protocol stack (BlueZ)](http://www.bluez.org/) for [Rust](https://www.rust-lang.org/).
Both publishing local and consuming remote GATT services using *idiotmatic* Rust code is supported.
Asynchronous support is depended by [Tokio](https://tokio.rs/).

This project started as a fork of [blurz](https://github.com/szeged/blurz) but has 
since then become a full rewrite.

The following features are provided:

* adapter enumeration
    * configuration of power, discoverability, name, etc.
    * hotplug support through change events stream
* device discovery and querying of their properties
    * querying of address, name, class, signal strength (RSSI), etc.
    * Bluetooth Low Energy advertisements
    * change events stream
    * connecting and pairing
* consumption of remote GATT services
    * GATT service discovery
    * read, write and notify operations on characteristics
    * read and write operations on characteristic descriptors
    * optional use of low-overhead `AsyncRead` and `AsyncWrite` streams for notify and write operations
* publishing local GATT services
    * read, write and notify operations on characteristics
    * read and write operations on characteristic descriptors
    * two programming models supported
        * callback-based interface
        * low-overhead `AsyncRead` and `AsyncWrite` streams
* sending Bluetooth Low Energy advertisements

Classic Bluetooth is unsupported except for device discovery.

Supported BlueZ versions
------------------------

This libray has been tested with BlueZ version 5.56.

Older versions might work, but be aware that many bugs related to GATT handling exist.
Refer to the [official changelog](https://github.com/bluez/bluez/blob/master/ChangeLog) for details.

Examples
--------
Refer to the API documentation and `examples` folder for examples.
