//! LE Passive Scan & Subscribe to updates for discovered peripheral(s).
//!
//! Usage: cargo run  --features=bluetoothd --example le_passive_scan <device> <AD type> <offset> <filter byte>..
//! Example: should subscribe to all devices advertising manufacturer data with manufacturer id 0xffff (default/unassigned)
//!        cargo run  --features=bluetoothd --example le_passive_scan hci0 255 00 0xff 0xff

use bluer::monitor::{Monitor, MonitorEvent, Pattern, RssiSamplingPeriod};
use futures::StreamExt;

fn parse_u8_maybe_hex(s: &str) -> Result<u8, std::num::ParseIntError> {
    if s.starts_with("0x") {
        u8::from_str_radix(&s[2..], 16)
    } else {
        s.parse()
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> bluer::Result<()> {
    env_logger::init();

    let adapter_name = std::env::args().nth(1);
    let data_type: u8 = match std::env::args().nth(2) {
        Some(s) => parse_u8_maybe_hex(&s).expect("Failed to parse AD type"),
        None => 0xff,
    };
    let start_position: u8 = match std::env::args().nth(3) {
        Some(s) => parse_u8_maybe_hex(&s).expect("Failed to parse or-pattern start position"),
        None => 0x00,
    };
    let filter_string: Vec<String> = std::env::args().skip(4).collect();
    let content: Vec<u8> = if filter_string.len() > 0 {
        filter_string.iter().map(|s| parse_u8_maybe_hex(s).expect("Failed to parse or-pattern data")).collect()
    } else {
        vec![0xff, 0xff]
    };

    if content.len() == 0 {
        panic!("No filter bytes provided");
    }

    let pattern = Pattern { data_type, start_position, content };

    let session = bluer::Session::new().await?;

    let adapter = match adapter_name {
        Some(name) => session.adapter(&name)?,
        None => session.default_adapter().await?,
    };
    println!("Running le_passive_scan on adapter {} with or-pattern {:?}", adapter.name(), pattern);

    adapter.set_powered(true).await?;

    let mm = adapter.monitor().await?;
    let mut monitor_handle = mm
        .register(Monitor {
            monitor_type: bluer::monitor::Type::OrPatterns,
            rssi_low_threshold: None,
            rssi_high_threshold: None,
            rssi_low_timeout: None,
            rssi_high_timeout: None,
            rssi_sampling_period: Some(RssiSamplingPeriod::First),
            patterns: Some(vec![pattern]),
            ..Default::default()
        })
        .await?;

    while let Some(mevt) = &monitor_handle.next().await {
        if let MonitorEvent::DeviceFound(devid) = mevt {
            println!("Discovered device {:?}", devid);
            let dev = adapter.device(devid.device)?;
            tokio::spawn(async move {
                let mut events = dev.events().await.unwrap();
                while let Some(ev) = events.next().await {
                    println!("On device {:?}, received event {:?}", dev, ev);
                }
            });
        }
    }

    Ok(())
}
