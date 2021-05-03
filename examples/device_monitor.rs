//! Scans for and monitors Bluetooth devices.

use std::{
    collections::{HashMap, VecDeque},
    io::stdout,
    time::{Duration, Instant},
};
use crossterm::{
    cursor, execute,
    style::{self, Colorize},
    terminal::{self, ClearType},
};
use futures::{pin_mut, stream::SelectAll, StreamExt};
use tokio::time::sleep;

use blurz::{Adapter, Address, DeviceChanged, DeviceEvent, DiscoveryFilter};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const MAX_AGO: u64 = 30;

async fn device_line(adapter: &Adapter, addr: Address, seen_ago: u64) -> Result<String> {
    use std::fmt::Write;
    let mut line = String::new();
    let device = adapter.device(addr)?;

    write!(&mut line, "{}", addr.to_string().white())?;
    write!(&mut line, " [{}] ", device.address_type().await?)?;

    const MIN_RSSI: i16 = -120;
    const MAX_RSSI: i16 = -30;
    const MAX_BAR_LEN: i16 = 25;
    let bar_len = if let Some(rssi) = device.rssi().await? {
        write!(&mut line, "{} dBm [", format!("{:4}", rssi).red())?;
        (rssi.clamp(MIN_RSSI, MAX_RSSI) - MIN_RSSI) * MAX_BAR_LEN / (MAX_RSSI - MIN_RSSI)
    } else {
        write!(&mut line, "---- dBm [")?;
        0
    };
    write!(&mut line, "{}", "#".repeat(bar_len as _).black().on_red())?;
    write!(&mut line, "{}", " ".repeat((MAX_BAR_LEN - bar_len) as _))?;
    write!(&mut line, "]")?;

    const MAX_AGO_BAR_LEN: u64 = 10;
    let ago_bar_len = (MAX_AGO - seen_ago.clamp(0, MAX_AGO)) * MAX_AGO_BAR_LEN / MAX_AGO;
    write!(&mut line, "{} s ago [", format!("{:3}", seen_ago).green())?;
    write!(
        &mut line,
        "{}",
        "#".repeat(ago_bar_len as _).black().on_green()
    )?;
    write!(
        &mut line,
        "{}",
        " ".repeat((MAX_AGO_BAR_LEN - ago_bar_len) as _)
    )?;
    write!(&mut line, "]")?;

    write!(
        &mut line,
        "  {}   ",
        format!("{:30}", device.name().await?.unwrap_or_default()).blue()
    )?;

    Ok(line)
}

fn clear_line(row: u16) {
    execute!(
        stdout(),
        cursor::MoveTo(0, row),
        terminal::DisableLineWrap,
        terminal::Clear(ClearType::CurrentLine)
    )
    .unwrap();
}

async fn show_device(adapter: &Adapter, addr: Address, seen_ago: Duration, row: u16) {
    let line = device_line(adapter, addr, seen_ago.as_secs())
        .await
        .unwrap_or_else(|err| {
            format!(
                "{} - Error: {}",
                addr.to_string().white(),
                err.to_string().on_dark_red()
            )
        });

    let (_, n_rows) = terminal::size().unwrap();
    clear_line(row);
    execute!(
        stdout(),
        cursor::Hide,
        cursor::MoveTo(0, row),
        style::Print(line),
        cursor::MoveTo(0, n_rows - 2),
        cursor::Show,
    )
    .unwrap();
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let session = blurz::Session::new().await?;
    let adapter_names = session.adapter_names().await?;
    let adapter_name = adapter_names.first().expect("No Bluetooth adapter present");

    execute!(
        stdout(),
        terminal::Clear(ClearType::All),
        cursor::MoveTo(0, 0),
        style::Print(format!(
            "Discovering devices using Bluetooth adapater {}",
            adapter_name.clone().blue()
        ))
    )
    .unwrap();

    let adapter = session.adapter(&adapter_name)?;
    let _discovery_session = adapter.discover_devices(DiscoveryFilter::default()).await?;

    let mut all_change_events = SelectAll::new();
    let device_events = adapter.device_changes().await?;
    pin_mut!(device_events);

    let (_, n_rows) = terminal::size()?;
    let mut empty_rows: VecDeque<u16> = (2..n_rows - 1).collect();
    let mut addr_row: HashMap<Address, u16> = HashMap::new();
    let mut last_seen: HashMap<Address, Instant> = HashMap::new();

    let next_update = sleep(Duration::from_secs(1));
    pin_mut!(next_update);

    loop {
        tokio::select! {
            Some(device_event) = device_events.next() => {
                match device_event {
                    DeviceEvent::Added(addr) => {
                        if let Some(row) = empty_rows.pop_front() {
                            let device = adapter.device(addr)?;
                            all_change_events.push(device.changes().await?);

                            addr_row.insert(addr, row);
                            last_seen.insert(addr, Instant::now());
                            show_device(&adapter, addr, Duration::default(), row).await;
                        }
                    }
                    DeviceEvent::Removed(addr) => {
                        if let Some(row) = addr_row.remove(&addr) {
                            clear_line(row);
                            empty_rows.push_back(row);
                            empty_rows.make_contiguous().sort();
                            last_seen.remove(&addr);
                        }
                    }
                }
            },
            Some(DeviceChanged { address: addr, ..}) = all_change_events.next() => {
                if let Some(row) = addr_row.get(&addr) {
                    last_seen.insert(addr, Instant::now());
                    show_device(&adapter, addr, Duration::default(), *row).await;
                }
            },
            _ = &mut next_update => {
                for (addr, row) in addr_row.clone().iter() {
                    if let Some(ls) = last_seen.get(&addr) {
                        let seen_ago = ls.elapsed();
                        if seen_ago.as_secs() > MAX_AGO {
                            clear_line(*row);
                            addr_row.remove(&addr);
                            empty_rows.push_back(*row);
                            empty_rows.make_contiguous().sort();
                            last_seen.remove(&addr);
                        } else {
                            show_device(&adapter, *addr, seen_ago, *row).await;
                        }
                    }
                }
            },
            else => break,
        }
    }

    Ok(())
}
