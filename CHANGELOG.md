# Changelog
All notable changes to blez will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## 0.8.1 - unreleased
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
