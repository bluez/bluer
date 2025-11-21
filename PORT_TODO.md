# Porting BlueR from dbus to zbus

## Strategy
- [x] Disable all modules depending on dbus
- [x] Disable bluer-tools by removing it from the workspace
- [x] Make lib.rs compile by commenting all dependencies inside it
- [x] Port lib.rs
- [x] Reenable and port session.rs
- [x] Reenable and port adapter.rs
- [x] Reenable and port adv.rs
- [x] Reenable and port agent.rs
- [x] Reenable and port monitor.rs
- [x] Reenable and port gatt/mod.rs and gatt/remote.rs
- [x] Reenable and port gatt/local.rs
- [x] Reenable and port rfcomm/profile.rs
- [x] Reenable and port device.rs
- [x] Reenable and port bluer-tools
- [x] Reenable and port mesh module

## Current Status
- Porting complete. Verification needed.

## Known Issues
- `gatt_echo_client` stress test fails with `UnexpectedEof` and timeouts (exit code 124) on some systems.
  - Cause: The `gatt_echo_server` uses a bounded channel (`mpsc::channel(50)`) which can cause the reader loop to block if the writer loop is slow, leading to L2CAP buffer overflow and connection drop.
  - Potential Fix: Switch `gatt_echo_server` to `mpsc::unbounded_channel()` and increase the read buffer size.
  - Workaround: Reduce test load in `gatt_echo_client` (iterations: 50, max size: 20KB).

