//! Perform a Bluetooth LE advertisement.

use blez::LeAdvertisement;
use tokio::io::{AsyncBufReadExt, BufReader};

#[tokio::main(flavor = "current_thread")]
async fn main() -> blez::Result<()> {
    let session = blez::Session::new().await?;
    let adapter_names = session.adapter_names().await?;
    let adapter_name = adapter_names.first().expect("No Bluetooth adapter present");
    let adapter = session.adapter(&adapter_name)?;

    println!("Advertising on Bluetooth adapter {}", &adapter_name);

    let le_advertisement = LeAdvertisement {
        advertisement_type: blez::LeAdvertisementType::Peripheral,
        //manufacturer_data: vec![(123, vec![1, 2, 3])].into_iter().collect(),
        service_uuids: vec!["123e4567-e89b-12d3-a456-426614174000".parse().unwrap()].into_iter().collect(),
        //solicit_uuids: vec!["123e4567-e89b-12d3-a456-426614174111".parse().unwrap()].into_iter().collect(),
        discoverable: Some(true),
        local_name: Some("le_advertise".to_string()),
        ..Default::default()
    };
    let handle = adapter.le_advertise(le_advertisement).await?;

    println!("Press enter to quit");
    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();
    let _ = lines.next_line().await;

    println!("Removing advertisement");
    drop(handle);

    Ok(())
}
