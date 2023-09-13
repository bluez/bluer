# Changelog
All notable changes to BlueR will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## 0.16.1 - 2023-09-13
### Added
- expose MTU of remote GATT characteristics

## 0.16.0 - 2023-07-19
### Added
- Experimental Bluetooth mesh support
- Bluetooth advertisement monitor
- Bluetooth discovery filter
### Changed
- minimum supported Rust version is 1.70.0

## 0.15.7 - 2023-01-31
### Added
- conversion from Error to std::io::Error

## 0.15.6 - 2023-01-30
### Added
- string representation for RFCOMM socket address

## 0.15.5 - 2023-01-11
### Added
- Convert Bluetooth address to and from MacAddr6.

## 0.15.4 - 2022-12-14
### Added
- expose device making request in GATT server
### Fixed
- fix AdvertisingData handling by jaqchen

## 0.15.3 - 2022-11-30
### Fixed
- documentation build on docs.rs

## 0.15.2 - 2022-11-30
### Fixed
- use proper D-Bus type for advertising_data by Paul Otto
- fix musl libc build by Luca Barbato
- fix register_agent documentation by Patrick Flakus
- fix a typo by Mariusz Białończyk
### Changed
- update dependencies

## 0.15.1 - 2022-10-02
### Fixed
- correct D-Bus AuthorizeService name by hugmanrique
### Changed
- limit nix features by rtzoeller

## 0.15.0 - 2022-04-21
### Changed
- Update uuid crate to 1.0.

## 0.14.0 - 2022-04-07
### Added
- Implement Serde traits on types where possible, gated by `serde` feature.
- `all_properties` method on `Adapter`, `Device`, `Service` and `Characteristic`
  to query the values of all properties and return them as a `Vec<xxxProperty>`.
### Changed
- Crate features are now opt-in; use the `full` feature to
  enable all features.
- MSRV is now 1.60 to make use of namespaced Cargo features.
- Update Bluetooth numbers database.

## 0.13.3 - 2022-02-07
### Fixed
- Use a{sv} signature for ConnectDevice call.

## 0.13.2 - 2022-01-24
### Added
- implement from_raw_fd for socket types
- Session::default_adapter
### Fixed
- avoid memory/resource leak when dropping Session
- make descriptor offset property optional

## 0.13.1 - 2021-12-16
### Changed
- fix manufacturer data property change events not working

## 0.13.0 - 2021-11-11
### Added
- RFCOMM sockets
- RFCOMM profiles
- service classes id database
### Changed
- non-exhaustive attributes on structs for future-proofing

## 0.12.0 - 2021-11-09
### Added
- AddressType::BrEdr for classic Bluetooth
- Bluetooth classic support for L2CAP sockets
- l2cap: more socket options
### Changed
- rename LE AddressTypes

## 0.11.1 - 2021-10-21
### Changed
- fix `PSM_DYN_START` to be u16

## 0.11.0 - 2021-10-21
### Changed
- fix `Appearance` device property to be u16 by Daniel Thoma
- change PSM to be u16 for compatibility with classic Bluetooth
- update Bluetooth numbers database
- update dependencies

## 0.10.4 - 2021-08-26
### Changed
- gatt: Call socketpair() more idiomatically by André Zwing,
  allowing build with Rust 1.50
- fix clippy warnings

## 0.10.3 - 2021-07-18
### Added
- Tokio crate feature dependency `io-util`

## 0.10.2 - 2021-07-06
### Removed
- dependency on `libbluetooth`

## 0.10.1 - 2021-07-06
### Changed
- make database of assigned numbers optional

## 0.10.0 - 2021-07-06
### Changed
- rename project to BlueR

## 0.9.6 - 2021-06-30
### Changed
- clarify use of `advertise` method for LE advertisements
- update `dbus-crossroads` crate to 0.4

## 0.9.5 - 2021-06-21
### Added
- Bluetooth Authorization Agent API
### Removed
- Device::cancel_pairing was replaced by automatic cancellation.

## 0.9.4 - 2021-06-14
### Changed
- reduce crate size

## 0.9.3 - 2021-06-14
### Changed
- documentation fixes

## 0.9.2 - 2021-06-14
### Changed
- Move tools to `blez-tools` crate.

## 0.9.1 - 2021-06-14
### Added
- database of assigned number
- `gattcat` tool functionality.
- Manufacturer info in `blemon` tool.
### Removed
- `Descriptor::flags` because it is not provided by BlueZ.

## 0.9.0 - 2021-06-08
### Added
- L2CAP sockets.
- `l2cap_client` and `l2cap_server` examples.
- `gatt_echo_client` and `gatt_echo_server` examples.
- `send`, `try_send` and `sendable` methods on `CharacteristicWriter`.
- `recv`, `try_recv` and `recvable` methods on `CharacteristicReader`.
- `l2cat` and `gattcat` examples.
### Changed
- Allow data larger than MTU when using `AsyncWrite` on `CharacteristicWriter`.
- Allow buffers smaller than MTU when using `AsyncRead` on `CharacteristicReader`.
- Provide `AsRawFd` and `IntoRawFd` on `CharacteristicReader` and `CharacteristicWriter`
  instead of UNIX socket access.
- Close notification for `CharacteristicWriter`.

## 0.8.1 - 2021-06-06
### Added
- `--changes` switch to `discover_devices` example.
### Changed
- Make event dispatching O(1).
  (was O(n) where n is number of subscriptions)

## 0.8.0 - 2021-05-31
### Added
- `gatt_server_io` example.
### Changed
- `CharacteristicControl` provides an event stream instead of separate functions
  for notify and write requests.
- Make D-Bus errors internal.
- Fixed typos.

## 0.7.1 - 2021-05-30
### Changed
- Documentation updates.

## 0.7.0 - 2021-05-30
### Added
- `Includes` in remote GATT service.
### Changed
- Separate internal errors.
- Move advertisements to own module `adv`.
- Rename `Reject` to `ReqError`.
- Improve documentation.
### Removed
- `Handle` in remote GATT service.


## 0.6.0 - 2021-05-28
### Changed
- Initial version published on [crates.io](https://crates.io).
