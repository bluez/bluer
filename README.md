BLEZ - Asynchronous Bluetooth Low Energy on Linux for Rust
==========================================================

[![crates.io page](https://img.shields.io/crates/v/blez)](https://crates.io/crates/blez)
[![docs.rs page](https://docs.rs/blez/badge.svg)](https://docs.rs/blez)
[![BSD-2-Clause license](https://img.shields.io/crates/l/blez)](https://github.com/surban/blez/blob/master/LICENSE)

This library provides an asynchronous, fully featured interface to the [Bluetooth Low Energy (BLE)](https://en.wikipedia.org/wiki/Bluetooth_Low_Energy)
APIs of the [official Linux Bluetooth protocol stack (BlueZ)](http://www.bluez.org/) for [Rust](https://www.rust-lang.org/).
Both publishing local and consuming remote [GATT services](https://www.oreilly.com/library/view/getting-started-with/9781491900550/ch04.html) using *idiomatic* Rust code is supported.
L2CAP sockets are presented using an API similar to [Tokio](https://tokio.rs/) networking.

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

History
-------

This project started as a fork of [blurz](https://github.com/szeged/blurz) but has
since then become a full rewrite.
Documentation has been mostly copied from the
[BlueZ API specification](https://git.kernel.org/pub/scm/bluetooth/bluez.git/tree/doc/), but
also adapted where it makes sense.

Crate features
--------------
All crate features are enabled by default.

* `bluetoothd`: Enables all functions requiring a running Bluetooth daemon.
  For building, D-Bus library headers must be installed.
* `l2cap`: Enables L2CAP sockets.

Requirements
------------

This library has been tested with BlueZ version 5.58 with [additional patches](https://github.com/surban/bluez/tree/all-fixes) applied.
Older versions might work, but be aware that many bugs related to GATT handling exist.
Refer to the [official changelog](https://github.com/bluez/bluez/blob/master/ChangeLog) for details.

If any `bluetoothd` feature is used the Bluetooth daemon must be running and configured for access over D-Bus.
On most distributions this should work out of the box.

For building, D-Bus library headers must be installed if the `bluetoothd` feature is enabled.
On Debian-based distributions install the package `libdbus-1-dev`.

Configuration
-------------

The following options in `/etc/bluetooth/main.conf` are helpful.

    [GATT]
    Cache = no
    Channels = 1

This disables the GATT cache to avoid stale data during device discovery.

By only allowing one channel the extended attribute protocol (EATT) is disabled.
If EATT is enabled, all GATT commands and notifications are sent over multiple L2CAP channels and can be reordered arbitrarily by lower layers of the protocol stack.
This makes sequential data transmission over GATT characteristics more difficult.

Troubleshooting
---------------

The library returns detailed errors received from BlueZ.

Set the Rust log level to `trace` to see all D-Bus communications with BlueZ.

In some cases checking the Bluetooth system log might provide further insights.
On Debian-based systems it can be displayed by executing `journalctl -u bluetooth`.
Check the `bluetoothd` man page for increasing the log level.

Sometimes deleting the system Bluetooth cache at `/var/lib/bluetooth` and restarting
`bluetoothd` is helpful.

Examples
--------
Refer to the [API documentation](https://docs.rs/blez) and
[examples folder](https://github.com/surban/blez/tree/master/examples) for examples.

The following example applications are provided.

  - **discover_devices**: Discover Bluetooth devices and print their properties.

  - **gatt_client**: Simple GATT client that calls read, write and notify on a characteristic.

  - **gatt_server_cb**: Corresponding GATT server implemented using callback programming model.

  - **gatt_server_io**: Corresponding GATT server implemented using IO programming model.

  - **gatt_echo_client**: Simple GATT client that connects to a server and sends and receives test data.

  - **gatt_echo_server**: Corresponding GATT server that echos received data.

  - **l2cap_client**: Simple L2CAP socket client that connects to a socket and sends and receives test data.

  - **l2cap_server**: Corresponding L2CAP socket server that echos received data.

  - **le_advertise**: Register Bluetooth LE advertisement.

  - **list_adapters**: List installed Bluetooth adapters and their properties.

Use `cargo run --example <name>` to run a particular example application.

Tools
-----

The following tools are included and also serve as examples.

  - **blemon**: Scans for and monitors Bluetooth LE devices similar to `top`.

  - **gattcat**: `netcat`-like for GATT characteristics.

  - **l2cat**: `netcat`-like for L2CAP sockets.

Use `cargo install blez --example <name>` to install a tool on your system.

