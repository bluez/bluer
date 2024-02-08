//! Scans for and monitors Bluetooth devices.

use bluer::{id, Adapter, AdapterEvent, Address};
use crossterm::{
    cursor, execute, queue,
    style::{self, Stylize},
    terminal::{self, ClearType},
};
use futures::{pin_mut, FutureExt, StreamExt};
use std::{
    collections::HashMap,
    convert::TryFrom,
    io::{stdout, SeekFrom, Seek},
    iter,
    time::{Duration, Instant}, path::Path,
};

use tokio::time::sleep;

use serde::{Serialize, Deserialize};
use std::fs::{OpenOptions, File};
use std::io::{Write, BufReader, BufRead};
use std::error::Error;

// Define the BluetoothAdvertisement struct
#[derive(Serialize, Deserialize, Debug)]

// This should include RSSI and the relevant bluer::adapter::Adapter fields
struct BluetoothAdvertisement {
    local_name: String,
    address: String,
    address_type: String,
    manufacturer_data: String,
    service_data: String,
    last_seen: i32,
    RSSI: i16,    
}

// A struct to manage the file operations
struct AdvertisementLogger {
    file: File,
}

impl AdvertisementLogger {
    // Function to create a new AdvertisementLogger
    fn new(file_name: &str) -> std::result::Result<Self, Box<dyn Error>> {
        let path = Path::new(file_name);
        let mut file = File::options()
            .create(true)
            .read(true)
            .write(true)
            .open(path)?;

        if path.metadata()?.len() == 0 {
            // If the file is new, start an array
            writeln!(file, "[")?;
        } else {
            // Move the file pointer before the last character (likely ']' or '\n')
            file.seek(SeekFrom::End(-1))?;
        }

        Ok(AdvertisementLogger { file })
    }

    // Function to append a BluetoothAdvertisement
    fn append(&mut self, adv: &BluetoothAdvertisement) -> std::result::Result<(), Box<dyn Error>> {
       // Serialize the BluetoothAdvertisement to JSON with formatting
       let json_str = serde_json::to_string_pretty(adv)?;

       // Check if the file already contains data
       let len = self.file.metadata()?.len();
       if len > 1 {
           // If so, overwrite the last ']' or '\n' with a comma and newline
           self.file.seek(SeekFrom::End(-1))?;
           writeln!(self.file, ",")?;
       }

       // Append the JSON string
       writeln!(self.file, "{}", json_str)?;

        Ok(())
    }

    // Function to properly close the file
    fn close(mut self) -> std::result::Result<(), Box<dyn Error>> {
        // Write the closing bracket for the JSON array
        writeln!(self.file, "\n]")?;
        Ok(())
    }
}


type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const MAX_AGO: u64 = 30;
const UPDATE_INTERVAL: Duration = Duration::from_secs(1);

fn clear_line(row: u16) {
    queue!(stdout(), cursor::MoveTo(0, row), terminal::DisableLineWrap, terminal::Clear(ClearType::CurrentLine))
        .unwrap();
}

struct DeviceMonitor {
    adapter: Adapter,
    n_rows: u16,
    empty_rows: Vec<u16>,
    devices: HashMap<Address, DeviceData>,
}

#[derive(Clone)]
struct DeviceData {
    address: Address,
    row: u16,
    last_seen: Instant,
}

impl DeviceMonitor {
    pub async fn run(adapter: Adapter) -> Result<()> {
        let (_, n_rows) = terminal::size()?;
        let mut this =
            Self { adapter, n_rows, empty_rows: (2..n_rows - 1).rev().collect(), devices: HashMap::new() };
        this.perform().await
    }

    async fn perform(&mut self) -> Result<()> {
        let device_events = self.adapter.discover_devices_with_changes().await?;
        pin_mut!(device_events);

        let mut next_update = sleep(UPDATE_INTERVAL).boxed();
        let mut logger = AdvertisementLogger::new("advertisements.json")?;

        loop {
            tokio::select! {
                Some(device_event) = device_events.next() => {
                    match device_event {
                        AdapterEvent::DeviceAdded(addr) => {
                            match self.devices.get_mut(&addr) {
                                Some(data) => data.last_seen = Instant::now(),
                                None => self.add_device(addr).await,
                            }
                        },
                        AdapterEvent::DeviceRemoved(addr) => self.remove_device(addr).await,
                        _ => (),
                    }
                },
                _ = &mut next_update => {
                    for (addr, data) in self.devices.clone().iter() {
                        let seen_ago = data.last_seen.elapsed();
                        if seen_ago.as_secs() > MAX_AGO {
                            self.remove_device(*addr).await;
                        } else {
                            self.show_device(data).await;
                        }

                        let device = self.adapter.device(data.address)?;

                        // Assuming `device.manufacturer_data().await?` returns a `HashMap<u16, Vec<u8>>`
                        let manufacturer_data_map = device.manufacturer_data().await?.unwrap_or_default();

                        // Convert the HashMap into a string representation
                        let manufacturer_data_string = manufacturer_data_map.iter()
                        .map(|(key, value)| {
                            // Convert key to hex string with "0x" prefix
                            let key_hex = format!("0x{:04X}", key);
                    
                            // Convert each byte in the vector to its hex representation with "0x" prefix
                            // If the byte is a printable ASCII character, also show the character in parentheses
                            let value_hex = value.iter()
                                .map(|byte| {
                                    let hex_str = format!("0x{:02X}", byte);
                                    if byte.is_ascii_graphic() || *byte == b' ' { // Includes space as printable
                                        format!("{}({})", hex_str, *byte as char)
                                    } else {
                                        hex_str
                                    }
                                })
                                .collect::<Vec<String>>()
                                .join(" ");
                    
                            format!("{}: [{}]", key_hex, value_hex)
                        })
                        .collect::<Vec<String>>()
                        .join(", ");

                        logger.append(&BluetoothAdvertisement {
                            address: device.address().to_string(),
                            address_type: device.address_type().await?.to_string(),
                            local_name: device.name().await?.unwrap_or_default(),
                            manufacturer_data: manufacturer_data_string,
                            service_data: String::new(), //device.service_data().await?.unwrap_or_default().to_string(),
                            RSSI: device.rssi().await?.unwrap_or_default(),
                            last_seen: data.last_seen.elapsed().as_secs() as i32,
                        })?;

                    }
                    next_update = sleep(UPDATE_INTERVAL).boxed();
                },
                else => break,
            }
        }

          // Properly close the file
        logger.close()?;

        Ok(())
    }

    async fn add_device(&mut self, address: Address) {
        if self.devices.contains_key(&address) {
            return;
        }
        if let Some(row) = self.empty_rows.pop() {
            self.devices.insert(address, DeviceData { address, row, last_seen: Instant::now() });

            self.show_device(&self.devices[&address]).await;
        }
    }

    async fn remove_device(&mut self, address: Address) {
        if let Some(data) = self.devices.remove(&address) {
            clear_line(data.row);
            self.empty_rows.push(data.row);
            self.empty_rows.sort_by(|a, b| b.cmp(a));
        }
    }

    async fn device_line(&self, data: &DeviceData) -> Result<String> {
        use std::fmt::Write;
        let mut line = String::new();
        let device = self.adapter.device(data.address)?;

        write!(&mut line, "{}", data.address.to_string().white())?;
        write!(&mut line, " [{}] ", device.address_type().await?)?;

        const MIN_RSSI: i16 = -120;
        const MAX_RSSI: i16 = -30;
        const MAX_BAR_LEN: i16 = 15;
        let bar_len = if let Some(rssi) = device.rssi().await? {
            write!(&mut line, "{} dBm [", format!("{rssi:4}").red())?;
            (rssi.clamp(MIN_RSSI, MAX_RSSI) - MIN_RSSI) * MAX_BAR_LEN / (MAX_RSSI - MIN_RSSI)
        } else {
            write!(&mut line, "---- dBm [")?;
            0
        };
        write!(&mut line, "{}", "#".repeat(bar_len as _).black().on_red())?;
        write!(&mut line, "{}", " ".repeat((MAX_BAR_LEN - bar_len) as _))?;
        write!(&mut line, "]")?;
        
        const MAX_AGO_BAR_LEN: u64 = 10;
        let seen_ago = data.last_seen.elapsed().as_secs();
        let ago_bar_len = (MAX_AGO - seen_ago.clamp(0, MAX_AGO)) * MAX_AGO_BAR_LEN / MAX_AGO;
        write!(&mut line, "{} s ago [", format!("{seen_ago:3}").green())?;
        write!(&mut line, "{}", "#".repeat(ago_bar_len as _).black().on_green())?;
        write!(&mut line, "{}", " ".repeat((MAX_AGO_BAR_LEN - ago_bar_len) as _))?;
        write!(&mut line, "]")?;

        let md = device.manufacturer_data().await?.unwrap_or_default();
        let mut m_ids: Vec<u16> = md.keys().cloned().collect();
        m_ids.sort_unstable();
        let ms: Vec<_> = m_ids
            .into_iter()
            .filter_map(|id| {
                id::Manufacturer::try_from(id)
                    .ok()
                    .map(|m| m.to_string().split(&[' ', ','][..]).next().unwrap().to_string())
            })
            .collect();
        let ms: String = ms.join(" / ").chars().chain(iter::repeat(' ')).take(12).collect();
        write!(&mut line, "  {}", ms.cyan())?;

        write!(&mut line, " {}   ", format!("{:30}", device.name().await?.unwrap_or_default()).blue())?;

        Ok(line)
    }

    async fn show_device(&self, data: &DeviceData) {
        let line = self.device_line(data).await.unwrap_or_else(|err| {
            format!("{} - Error: {}", data.address.to_string().white(), err.to_string().on_dark_red())
        });

        queue!(stdout(), cursor::Hide).unwrap();
        clear_line(data.row);
        execute!(
            stdout(),
            cursor::MoveTo(0, data.row),
            style::Print(line),
            cursor::MoveTo(0, self.n_rows - 2),
            cursor::Show,
        )
        .unwrap();


    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    env_logger::init();
    let session = bluer::Session::new().await?;
    let adapter = session.default_adapter().await?;

    execute!(
        stdout(),
        terminal::Clear(ClearType::All),
        cursor::MoveTo(0, 0),
        style::Print(format!("Discovering devices using Bluetooth adapter {}", adapter.name().blue()))
    )
    .unwrap();

    adapter.set_powered(true).await?;
    DeviceMonitor::run(adapter).await?;

    Ok(())
}
