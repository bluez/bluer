# Agent Information

## Running Examples
When running examples such as `discover_devices` or `adapter_events`, you need to run them with a timeout from the shell. Otherwise, they just continue running indefinitely.

Example:
```bash
timeout 10s cargo run --example discover_devices
```

## Testing LE Advertising
To test this, you can use the `ubupi5a.srv` (Advertiser) and `ubupi5b.srv` (Scanner) machines.

### Automated Testing Script
A Python script `test_advertising.py` is available to automate the process of syncing code, running the advertiser on one machine, and the scanner on the other. It handles SSH connections and timeouts automatically.

**Why this is needed:**
Manual testing using shell commands (e.g., running the advertiser in the background with `&`) proved unreliable. Issues included:
*   Difficulty in managing the lifecycle of the remote advertiser process (ensuring it stays running during the scan but terminates afterwards).
*   Timing coordination between the two machines.
*   Inconsistent output capture from background SSH sessions.

**Usage:**
```bash
# Sync code to remote machines first
.misc/sync.sh ubupi5a.srv
.misc/sync.sh ubupi5b.srv

# Run the test (defaults to using dev/bluer on both)
python3 test_advertising.py

# Run cross-version tests (Advertiser Dir, Scanner Dir)
# Example: Master Advertiser -> Zbus Scanner
python3 test_advertising.py dev/bluer-master dev/bluer
```

### Findings (Nov 2025)
Cross-testing between the `master` branch (known good) and the `zbus` port revealed:
1.  **Functionality**: The `zbus` port works for both advertising and scanning.
2.  **Timing**: The `zbus` port may require longer scan durations to reliably detect advertisements. The test script uses a 60s advertisement and 45s scan window.
3.  **Compatibility**:
    *   Master Advertiser -> Zbus Scanner: **Success**
    *   Zbus Advertiser -> Master Scanner: **Success**
    *   Zbus Advertiser -> Zbus Scanner: **Success** (occasional flakiness observed)

### Manual Testing
If you prefer manual testing:
1. Upload the code to the machines using rsync:
   ```bash
   .misc/sync.sh ubupi5a.srv
   .misc/sync.sh ubupi5b.srv
   ```
2. On `ubupi5a.srv`, run the `le_advertise` example.
3. On `ubupi5b.srv`, run the `discover_devices` example to see the advertisement.
