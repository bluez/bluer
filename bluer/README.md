BlueR — Official BlueZ Bindings for Rust
========================================

[![crates.io page](https://img.shields.io/crates/v/bluer)](https://crates.io/crates/bluer)
[![docs.rs page](https://docs.rs/bluer/badge.svg)](https://docs.rs/bluer)
[![BSD-2-Clause license](https://img.shields.io/crates/l/bluer)](https://raw.githubusercontent.com/bluez/bluer/master/LICENSE)

This library provides the official [Rust] interface to the [Linux Bluetooth protocol stack (BlueZ)].
Both publishing local and consuming remote [GATT services] using *idiomatic* Rust code is supported.
L2CAP and RFCOMM sockets are presented using an API similar to [Tokio] networking.

The following functionality is provided:

* Bluetooth adapters
    * enumeration
    * configuration of power, discoverability, name, etc.
    * hot-plug support through change events stream
* Bluetooth devices
    * discovery with custom filters
    * querying of address, name, class, signal strength (RSSI), etc.
    * Bluetooth Low Energy advertisements
    * change events stream
    * connecting and pairing
    * passive LE advertisement monitoring
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
* Bluetooth authorization agent
* efficient event dispatching
    * not affected by D-Bus match rule count
    * O(1) in number of subscriptions
* L2CAP sockets
    * support for both classic Bluetooth (BR/EDR) and Bluetooth LE
    * stream oriented
    * sequential packet oriented
    * datagram oriented
    * async IO interface with `AsyncRead` and `AsyncWrite` support
* RFCOMM sockets
    * support for classic Bluetooth (BR/EDR)
    * stream oriented
    * async IO interface with `AsyncRead` and `AsyncWrite` support
* database of assigned numbers
    * manufacturer ids
    * service classes, GATT services, characteristics and descriptors

Currently, some classic Bluetooth (BR/EDR) functionality is missing.
However, pull requests and contributions are welcome!

[Rust]: https://www.rust-lang.org/
[Linux Bluetooth protocol stack (BlueZ)]: http://www.bluez.org/
[GATT services]: https://www.oreilly.com/library/view/getting-started-with/9781491900550/ch04.html
[Tokio]: https://tokio.rs/

Usage
-----

To use BlueR as a library, run the following command in your project directory:

    cargo add -F full bluer

This will add the latest version of BlueR with all features enabled as a dependency to your `Cargo.toml`.

Crate features
--------------
The following crate features are available.

* `bluetoothd`: Enables all functions requiring a running Bluetooth daemon.
  For building, D-Bus library headers, provided by `libdbus-1-dev` on Debian, must be installed.
* `id`: Enables database of assigned numbers.
* `l2cap`: Enables L2CAP sockets.
* `rfcomm`: Enables RFCOMM sockets.
* `serde`: Enables serialization and deserialization of some data types.

To enable all crate features specify the `full` crate feature.

Requirements
------------

The minimum support Rust version (MSRV) is 1.60.

This library has been tested with [BlueZ 5.60].
Older versions might work, but be aware that many bugs related to GATT handling exist.
Refer to the [official changelog] for details.

If any `bluetoothd` feature is used the Bluetooth daemon must be running and configured for access over D-Bus.
On most distributions this should work out of the box.

[BlueZ 5.60]: http://www.bluez.org/release-of-bluez-5-60/
[official changelog]: https://github.com/bluez/bluez/blob/master/ChangeLog

Configuration
-------------

The following options in `/etc/bluetooth/main.conf` are helpful when working with GATT services.

    [GATT]
    Cache = no
    Channels = 1

This disables the GATT cache to avoid stale data during device discovery.

By only allowing one channel the extended attribute protocol (EATT) is disabled.
If EATT is enabled, all GATT commands and notifications are sent over multiple L2CAP channels and can be reordered arbitrarily by lower layers of the protocol stack.
This makes sequential data transmission over GATT characteristics more difficult.

Building
--------

When cloning this repository make sure to use the following command.
Otherwise the build will fail with file not found errors.

    git clone --recursive https://github.com/bluez/bluer.git

D-Bus development headers are required for building.

Troubleshooting
---------------

The library returns detailed errors received from BlueZ.

Set the Rust log level to `trace` to see all D-Bus communications with BlueZ.

In some cases checking the Bluetooth system log might provide further insights.
On Debian-based systems it can be displayed by executing `journalctl -u bluetooth`.
Check the `bluetoothd` man page for increasing the log level.

Sometimes deleting the system Bluetooth cache at `/var/lib/bluetooth` and restarting
`bluetoothd` fixes persistent issues with device connectivity.

Examples
--------
Refer to the [API documentation] and [examples folder] for examples.

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

Use `cargo run --all-features --example <name>` to run a particular example application.

[API documentation]: https://docs.rs/bluer
[examples folder]: https://github.com/bluez/bluer/tree/master/bluer/examples

Tools
-----

See the [BlueR tools] crate for tools that build on this library.

[BlueR tools]: https://crates.io/crates/bluer-tools

History
-------

This project started as a fork of [blurz] but has since then become a full rewrite.
It was published under the name `blez` before it was designated the official Rust
interface to BlueZ and renamed to BlueR.
Documentation has been mostly copied from the [BlueZ API specification], but
also adapted where it makes sense.

[blurz]: https://github.com/szeged/blurz
[BlueZ API specification]: https://git.kernel.org/pub/scm/bluetooth/bluez.git/tree/doc/
