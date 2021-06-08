BLEZ - Asynchronous Bluetooth Low Energy on Linux for Rust
==========================================================

This library provides an asynchronous, fully featured interface to the [Bluetooth Low Energy (BLE)](https://en.wikipedia.org/wiki/Bluetooth_Low_Energy)
APIs of the [official Linux Bluetooth protocol stack (BlueZ)](http://www.bluez.org/) for [Rust](https://www.rust-lang.org/).
Both publishing local and consuming remote [GATT services](https://www.oreilly.com/library/view/getting-started-with/9781491900550/ch04.html) using *idiomatic* Rust code is supported.
Asynchronous support is dependent by [Tokio](https://tokio.rs/).

This project started as a fork of [blurz](https://github.com/szeged/blurz) but has
since then become a full rewrite.
Documentation has been mostly copied from the
[BlueZ API specification](https://git.kernel.org/pub/scm/bluetooth/bluez.git/tree/doc/), but
also adapted where it makes sense.
L2CAP sockets are presented using an API similar to Tokio networking.

The following functionality is provided:

* Bluetooth adapters
    * enumeration
    * configuration of power, discoverability, name, etc.
    * hot-plug support through change events stream
* Bluetooth devices
    * discovery
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
* efficient event dispatching
    * not affected by D-Bus match rule count
    * O(1) in number of subscriptions
* L2CAP sockets
    * stream oriented
    * sequential packet oriented
    * datagram oriented
    * async IO interface with `AsyncRead` and `AsyncWrite` support

Classic Bluetooth is unsupported except for device discovery.

Feature flags
-------------
All features are enabled by default.

* `bluetoothd`: Enables all functions requiring a running `bluetoothd`.
  For building, D-Bus library headers must be installed.
* `l2cap`: Enables L2CAP sockets.

Requirements
------------

This library has been tested with BlueZ version 5.56.
Older versions might work, but be aware that many bugs related to GATT handling exist.
Refer to the [official changelog](https://github.com/bluez/bluez/blob/master/ChangeLog) for details.

If any `bluetoothd` feature is used the `bluetoothd` daemon must be running and configured for access over D-Bus.
On most distributions this should work out of the box.

For building, D-Bus library headers must be installed if the `bluetoothd` feature is enabled.
On Debian-based distributions install the package `libdbus-1-dev`.

Troubleshooting
---------------

The library returns detailed errors received from BlueZ.

Set the Rust log level to `trace` to see all D-Bus communications with BlueZ.

In some cases checking the Bluetooth system log might provide further insights.
On Debian-based systems it can be displayed by executing `journalctl -u bluetooth`.
Check the `bluetoothd` man page for increasing the log level.

Examples
--------
Refer to the [API documentation](https://docs.rs/blez) and
[examples folder](https://github.com/surban/blez/tree/master/examples) for examples.

The following example applications are provided.

  - **device_monitor**: Scans for and monitors Bluetooth devices similar to `top`.

  - **discover_devices**: Discover Bluetooth devices and print their properties.

  - **gatt_client**: Simple GATT client that calls read, write and notify on a characteristic.

  - **gatt_server_cb**: Corresponding GATT server implemented using callback programming model.

  - **gatt_server_io**: Corresponding GATT server implemented using IO programming model.

  - **le_advertise**: Register Bluetooth LE advertisement.

  - **list_adapters**: List installed Bluetooth adapters and their properties.

Use `cargo run --example <name>` to run a particular example application.

