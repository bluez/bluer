//! LE Passive Scan & Subscribe to updates for discovered peripheral(s).

use bluer::monitor::{Monitor, MonitorEvent, Pattern, RssiSamplingPeriod};
use futures::StreamExt;

#[tokio::main(flavor = "current_thread")]
async fn main() -> bluer::Result<()> {
    env_logger::init();
    let session = bluer::Session::new().await?;
    let adapter = session.default_adapter().await?;
    let mm = adapter.monitor().await?;
    adapter.set_powered(true).await?;
    let mut monitor_handle = mm
        .register(Monitor {
            monitor_type: bluer::monitor::Type::OrPatterns,
            rssi_low_threshold: None,
            rssi_high_threshold: None,
            rssi_low_timeout: None,
            rssi_high_timeout: None,
            rssi_sampling_period: Some(RssiSamplingPeriod::First),
            patterns: Some(vec![Pattern { data_type: 0xff, start_position: 0x00, content: vec![0x99, 0x04] }]),
            ..Default::default()
        })
        .await?;

    while let Some(mevt) = &monitor_handle.next().await {
        if let MonitorEvent::DeviceFound(devid) = mevt {
            log::info!("Discovered device {:?}", devid);
            let dev = adapter.device(devid.device)?;
            tokio::spawn(async move {
                let mut events = dev.events().await.unwrap();
                while let Some(ev) = events.next().await {
                    log::info!("On device {:?}, received event {:?}", dev, ev);
                }
            });
        }
    }

    Ok(())
}
