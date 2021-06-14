# Changelog
All notable changes to blez will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## 0.9.2 - 2021-06-14
### Added
- gattcat: Nordic UART Service (NUS) 
- gattcat: list adapters
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
