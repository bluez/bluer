//! Perform a Bluetooth LE advertisement.

use blurz::LeAdvertisement;
use tokio::io::{AsyncBufReadExt, BufReader};

#[tokio::main(flavor = "current_thread")]
async fn main() -> blurz::Result<()> {
    let session = blurz::Session::new().await?;
    let adapter_names = session.adapter_names().await?;
    let adapter_name = adapter_names.first().expect("No Bluetooth adapter present");
    let adapter = session.adapter(&adapter_name)?;

    println!("Advertising on Bluetooth adapter {}", &adapter_name);

    let le_advertisement =
        LeAdvertisement { advertisement_type: blurz::LeAdvertisementType::Peripheral, ..Default::default() };
    let handle = adapter.le_advertise(le_advertisement).await?;

    println!("Press enter to quit");
    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();
    let _ = lines.next_line().await;

    println!("Removing advertisement");
    drop(handle);

    Ok(())
}
