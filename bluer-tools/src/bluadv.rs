//! Perform a Bluetooth LE advertisement.

use std::collections::BTreeSet;
use std::process;
use std::time::Duration;
use std::{error::Error, fmt, str::FromStr};

use bluer::{adv::Advertisement, adv::Type, Uuid};
use regex::Regex;
use structopt::StructOpt;
use tokio::signal::unix::{signal, SignalKind};
use tokio::time::sleep;

#[derive(Debug)]
struct IntervalParseError(String);

impl fmt::Display for IntervalParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Error for IntervalParseError {}

#[derive(Debug)]
struct Interval {
    min_milliseconds: u64,
    max_milliseconds: u64,
}

impl FromStr for Interval {
    type Err = IntervalParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split(',').collect();
        print!("parts = {:?}, s = {:?}", parts, s);
        if parts.len() != 2 {
            return Err(IntervalParseError("Expected two comma-separated numbers".into()));
        }
        // convert milliseconds to seconds
        let min =
            parts[0].parse::<u64>().map_err(|_| IntervalParseError("Failed to parse min interval".into()))?;
        let max =
            parts[1].parse::<u64>().map_err(|_| IntervalParseError("Failed to parse max interval".into()))?;
        Ok(Interval { min_milliseconds: min, max_milliseconds: max })
    }
}

// cargo run --  --advertisement-type "broadcast" --local-name "rust!"

//use uuid::Uuid; // Make sure the `uuid` crate is added to your dependencies
//use std::str::FromStr;

#[derive(Debug, StructOpt)]
#[structopt(name = "le_advertise", about = "A command tool to generate BLE advertisements")]
struct Opt {
    #[structopt(long, help = "Type of the advertisement", possible_values = &["broadcast", "peripheral"])]
    advertisement_type: Option<String>,

    /// Activate verbose mode
    // short and long flags (-v, --verbose) will be deduced from the field's name
    #[structopt(
        short,
        long,
        help = "Show detailed information for troubleshooting, including details about the adapters"
    )]
    verbose: bool,

    // short and long flags (-a, --advertiser) will be deduced from the field's name
    #[structopt(
        short,
        long,
        default_value = "",
        help = "The advertisement address in the form XX:XX:XX:XX:XX:XX  ex: 5C:F3:70:A1:71:0F"
    )]
    advertiser: String,

    /// UUID service to add to the advertisement. ex: 123e4567-e89b-12d3-a456-426614174000
    #[structopt(short = "u", long, use_delimiter = true, help = "List of service UUIDs separated by commas")]
    service_uuids: Vec<String>,

    /// Local name to be used in the advertising report.
    #[structopt(short, long, help = "Local name for the advertisement")]
    local_name: Option<String>,

    /// Advertise as general discoverable.
    #[structopt(long, help = "Advertise as general discoverable")]
    discoverable: bool,

    /// Duration of the advertisement in seconds.
    #[structopt(long, help = "Duration of the advertisement in seconds")]
    duration: Option<u64>,

    #[structopt(long, help = "Min and max advertising intervals in milliseconds. Example: --interval 30,60")]
    interval: Option<Interval>,

    #[structopt(long, help = "Advertising TX power level")]
    tx_power: Option<i16>,
}

impl Opt {
    fn validate(&self) {
        let re = Regex::new(r"^[0-9A-Fa-f]{2}(:[0-9A-Fa-f]{2}){5}$").unwrap();
        // verify the the advertising interval's minimum value is less than its maximum
        if let Some(ref interval) = self.interval {
            if interval.min_milliseconds >= interval.max_milliseconds {
                eprintln!(
                    "Invalid advertising interval. The minimum value should be less than the maximum value"
                );
                process::exit(1);
            }
        }

        if !self.advertiser.is_empty() && !re.is_match(&self.advertiser) {
            eprintln!("Invalid advertiser address format. It should be in the form XX:XX:XX:XX:XX:XX");
            process::exit(1);
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> bluer::Result<()> {
    // Support SIGTERM and SIGINT signals
    let mut sig_term = signal(SignalKind::terminate())?;
    let mut sig_int = signal(SignalKind::interrupt())?;

    let opt = Opt::from_args();
    opt.validate();
    let verbose = opt.verbose;
    env_logger::init();

    let service_uuids: BTreeSet<Uuid> = opt.service_uuids.iter().filter_map(|s| Uuid::from_str(s).ok()).collect();

    let session = bluer::Session::new().await?;
    // convert opt.advertisement_type to bluer::adv::Type
    let advertisement_type = match opt.advertisement_type.as_deref() {
        Some("broadcast") => Type::Broadcast,
        Some("peripheral") => Type::Peripheral,
        _ => Type::Peripheral,
    };

    let adapter_names = session.adapter_names().await?;
    let adapter_name = adapter_names.first().expect("No Bluetooth adapter present");
    let mut adapter = session.adapter(adapter_name)?;
    for adapter_name in adapter_names {
        let adapter_tmp = session.adapter(&adapter_name)?;
        let address = adapter_tmp.address().await?;

        if verbose {
            println!("Checking Bluetooth adapter {}: with an address {}", &adapter_name, address);
        }

        if opt.advertiser.is_empty() || address.to_string() == opt.advertiser {
            adapter = adapter_tmp;
            if verbose {
                println!("Using Bluetooth adapter {}", &adapter_name);
                println!("    Address: {}", address);
                // Print additional adapter details here as needed
            }
            break;
        }
    }

    adapter.set_powered(true).await?;
    if opt.verbose {
        println!("Advertising on Bluetooth adapter {}", adapter.name());
        println!("    Address:                    {}", adapter.address().await?);
        println!("    Address type:               {}", adapter.address_type().await?);
        println!("    Friendly name:              {}", adapter.alias().await?);
        println!("    System name:                {}", adapter.system_name().await?);
        println!("    Modalias:                   {:?}", adapter.modalias().await?);
        println!("    Powered:                    {:?}", adapter.is_powered().await?);
    }

    let le_advertisement = Advertisement {
        advertisement_type: advertisement_type,
        service_uuids,
        local_name: opt.local_name,
        discoverable: Some(opt.discoverable),
        duration: opt.duration.map(Duration::from_secs),
        tx_power: opt.tx_power,
        min_interval: opt.interval.as_ref().map(|i| Duration::from_millis(i.min_milliseconds)),
        max_interval: opt.interval.as_ref().map(|i| Duration::from_millis(i.max_milliseconds)),
        ..Default::default()
    };

    if verbose {
        println!("{:?}", &le_advertisement);
    }
    let handle = adapter.advertise(le_advertisement).await?;

    // Wait for a signal to stop the advertisement
    println!("Press enter to quit");

    // Wait for either a signal to stop the advertisement or user input
    tokio::select! {
        _ = sig_term.recv() => {
            if verbose {
                println!("SIGTERM received, shutting down gracefully...");
            }
        }
        _ = sig_int.recv() => {
            if verbose {
                println!("SIGINT received, shutting down gracefully...");
            }
        }

    }

    // Clean up and finish
    if verbose {
        println!("Removing advertisement");
    }
    drop(handle); // Ensure the advertisement is stopped

    sleep(Duration::from_secs(1)).await;
    if verbose {
        println!("Shutdown complete.");
    }

    Ok(())
}
