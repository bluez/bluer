//! Perform a Bluetooth LE advertisement.

use clap::{Parser, ValueEnum};
use std::{future::pending, str::FromStr, time::Duration};
use tokio::{
    signal::unix::{signal, SignalKind},
    time::sleep,
};

use bluer::{
    adv::{Advertisement, Type},
    Adapter, Address, Session, Uuid,
};

type AnyResult<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Advertisement type.
#[derive(Debug, Clone, Copy, ValueEnum)]
enum AdvertisementType {
    /// Broadcast.
    Broadcast,
    /// Peripheral.
    Peripheral,
}

impl From<AdvertisementType> for Type {
    fn from(value: AdvertisementType) -> Self {
        match value {
            AdvertisementType::Broadcast => Type::Broadcast,
            AdvertisementType::Peripheral => Type::Peripheral,
        }
    }
}

#[derive(Parser)]
#[clap(name = "bluadv", about = "Broadcast Bluetooth LE advertisements")]
struct Opt {
    /// Address of local Bluetooth adapter to use.
    #[clap(long, short)]
    bind: Option<Address>,

    /// Type of advertisement.
    #[clap(long, short = 't', default_value = "peripheral")]
    advertisement_type: AdvertisementType,

    /// Service UUID.
    ///
    /// Can be specified multiple times.
    #[clap(short = 'u', long)]
    service_uuid: Vec<Uuid>,

    /// Local name for the advertisement.
    #[clap(short = 'n', long)]
    local_name: Option<String>,

    /// Advertise as general discoverable.
    #[clap(short, long)]
    discoverable: bool,

    /// Duration of the advertisement in seconds.
    #[clap(long)]
    duration: Option<u64>,

    /// Minimum advertising interval in milliseconds.
    #[clap(long)]
    min_interval: Option<u64>,

    /// Maximum advertising interval in milliseconds.
    #[clap(long)]
    max_interval: Option<u64>,

    /// Advertising TX power level. The range is [-127 to +20] where units are in dBm.
    #[clap(long, short = 'p', allow_negative_numbers(true))]
    tx_power: Option<i16>,

    /// Manufacturer specific data in the form "<manufacturer id>:<hex data>" (manufacturer id is in hexadecimal).
    ///
    /// Can be specified multiple times.
    #[clap(long, short = 'm')]
    manufacturer_data: Vec<ManufacturerData>,

    /// Service data in the form "<service uuid>:<hex data>".
    ///
    /// Can be specified multiple times.

    #[clap(long, short = 's')]
    service_data: Vec<ServiceData>,

    /// Show detailed information.
    #[clap(short, long)]
    verbose: bool,

    /// Do not display exit prompt.
    #[clap(short, long)]
    quiet: bool,
}

#[derive(Clone)]
struct ManufacturerData {
    id: u16,
    data: Vec<u8>,
}

impl FromStr for ManufacturerData {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let Some((id, data)) = s.split_once(':') else {
            return Err(": missing".to_string());
        };
        Ok(Self {
            id: u16::from_str_radix(id, 16).map_err(|err| err.to_string())?,
            data: hex::decode(data).map_err(|err| err.to_string())?,
        })
    }
}

#[derive(Clone)]
struct ServiceData {
    id: Uuid,
    data: Vec<u8>,
}

impl FromStr for ServiceData {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let Some((id, data)) = s.split_once(':') else {
            return Err(": missing".to_string());
        };
        Ok(Self {
            id: id.parse::<Uuid>().map_err(|err| err.to_string())?,
            data: hex::decode(data).map_err(|err| err.to_string())?,
        })
    }
}

async fn get_session_adapter(addr: Option<Address>) -> AnyResult<(Session, Adapter)> {
    let session = bluer::Session::new().await?;
    let adapter_names = session.adapter_names().await?;

    match addr {
        Some(addr) => {
            for adapter_name in adapter_names {
                let adapter = session.adapter(&adapter_name)?;
                if adapter.address().await? == addr {
                    adapter.set_powered(true).await?;
                    return Ok((session, adapter));
                }
            }
            Err("specified Bluetooth adapter not present".into())
        }
        None => {
            let adapter_name = adapter_names.first().ok_or("no Bluetooth adapter present")?;
            let adapter = session.adapter(adapter_name)?;
            adapter.set_powered(true).await?;
            Ok((session, adapter))
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> AnyResult<()> {
    env_logger::init();
    let opt = Opt::parse();

    let mut sig_term = signal(SignalKind::terminate())?;
    let mut sig_int = signal(SignalKind::interrupt())?;

    let (_session, adapter) = get_session_adapter(opt.bind).await?;
    if opt.verbose {
        eprintln!("Using Bluetooth adapter {} with address {}", adapter.name(), adapter.address().await?);
    }

    let duration = opt.duration.map(Duration::from_secs);
    let timeout = async {
        match duration {
            Some(duration) => sleep(duration).await,
            None => pending().await,
        }
    };

    let le_advertisement = Advertisement {
        advertisement_type: opt.advertisement_type.into(),
        local_name: opt.local_name.clone(),
        discoverable: Some(opt.discoverable),
        duration,
        tx_power: opt.tx_power,
        min_interval: opt.min_interval.map(Duration::from_millis),
        max_interval: opt.max_interval.map(Duration::from_millis),
        manufacturer_data: opt.manufacturer_data.iter().map(|md| (md.id, md.data.clone())).collect(),
        service_data: opt.service_data.iter().map(|sd| (sd.id, sd.data.clone())).collect(),
        ..Default::default()
    };
    if opt.verbose {
        eprintln!("{le_advertisement:?}");
    }

    let handle = adapter.advertise(le_advertisement).await?;

    if !opt.quiet {
        eprintln!("Press <CTRL>-C to stop advertising");
    }

    // Wait for signal or timeout to stop the advertisement.
    tokio::select! {
        _ = sig_term.recv() => (),
        _ = sig_int.recv() => (),
        () = timeout => (),
    }

    // Clean up and finish
    if opt.verbose {
        eprintln!("Removing advertisement");
    }
    drop(handle);
    sleep(Duration::from_secs(1)).await;

    Ok(())
}
