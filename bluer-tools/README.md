BlueR tools — swiss army knife for GATT services and L2CAP sockets on Linux
===========================================================================

[![crates.io page](https://img.shields.io/crates/v/bluer-tools)](https://crates.io/crates/bluer-tools)
[![BSD-2-Clause license](https://img.shields.io/crates/l/bluer-tools)](https://raw.githubusercontent.com/bluez/bluer/master/LICENSE)

This crate provides tools for Bluetooth on Linux building on the functionality of the [BlueR crate].
A running [Bluetooth daemon (BlueZ)] is required.

The following command line tools are included.

  - **blumon**: Scans for and monitors Bluetooth devices similar to `top`.

  - **gattcat**: Swiss army knife for Bluetooth LE GATT services.
    - discovers Bluetooth LE devices and their services
    - pairing
    - resolves all well-known UUIDs and manufacturer ids
    - performs all possible operations on GATT services
    - connects (via notify and write) to a remote GATT service
    - serves (via notify and write) a local program over a GATT service
    - implements the [Nordic UART service (NUS)] as client and server

  - **l2cat**: [netcat]-like for Bluetooth LE L2CAP sockets.
    - connects to remote L2CAP PSMs
    - listens on local L2CAP PSMs and accepts connections
    - serves a local program on an L2CAP PSM

Each tool supports the `--help` option for detailed usage information.

[BlueR crate]: https://crates.io/crates/bluer
[Nordic UART service (NUS)]: https://developer.nordicsemi.com/nRF_Connect_SDK/doc/latest/nrf/include/bluetooth/services/nus.html
[netcat]: https://sectools.org/tool/netcat/
[Bluetooth daemon (BlueZ)]: http://www.bluez.org/

Installation
------------

First, install D-Bus and Bluetooth libraries on your system.
On Debian this can be achieved by running

    sudo apt install libdbus-1-dev

Then, run the following command to install BlueR tools

    cargo install bluer-tools

If you do not have Cargo on your system, you can use [rustup] for installing it.

[rustup]: https://rustup.rs/
