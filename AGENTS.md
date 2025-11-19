# Agent Information

## Running Examples
When running examples such as `discover_devices` or `adapter_events`, you need to run them with a timeout from the shell. Otherwise, they just continue running indefinitely.

Example:
```bash
timeout 10s cargo run --example discover_devices
```
