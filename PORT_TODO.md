# Porting BlueR from dbus to zbus

## Strategy
- [x] Disable all modules depending on dbus
- [x] Disable bluer-tools by removing it from the workspace
- [x] Make lib.rs compile by commenting all dependencies inside it
- [x] Port lib.rs
- [x] Reenable and port session.rs
- [ ] Reenable and port adapter.rs
- [ ] Reenable and port adv.rs
- [ ] Reenable and port agent.rs
- [ ] Reenable and port monitor.rs
- [ ] Reenable and port gatt/mod.rs and gatt/remote.rs
- [ ] Reenable and port gatt/local.rs
- [ ] Reenable and port rfcomm/profile.rs
- [ ] Reenable and port remaining modules
- [ ] Reenable and port bluer-tools

## Current Status
- Initializing port.
