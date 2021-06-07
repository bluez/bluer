//! Opens a listening L2CAP socket and accepts connections.

use blez::{
    adv::Advertisement,
    l2cap::{SocketAddr, StreamListener, PSM_DYN_START},
};
use std::time::Duration;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    time::sleep,
};

include!("gatt.inc");

#[tokio::main(flavor = "current_thread")]
async fn main() -> blez::Result<()> {
    env_logger::init();
    let session = blez::Session::new().await?;
    let adapter_names = session.adapter_names().await?;
    let adapter_name = adapter_names.first().expect("No Bluetooth adapter present");
    let adapter = session.adapter(&adapter_name)?;
    let adapter_addr = adapter.address().await?;
    let adapter_addr_type = adapter.address_type().await?;

    // Advertising is necessary for device to be connectable.
    println!(
        "Advertising on Bluetooth adapter {} with {} address {}",
        &adapter_name, &adapter_addr_type, &adapter_addr
    );
    let le_advertisement = Advertisement {
        service_uuids: vec![SERVICE_UUID].into_iter().collect(),
        discoverable: Some(true),
        local_name: Some("l2cap_server".to_string()),
        ..Default::default()
    };
    let adv_handle = adapter.advertise(le_advertisement).await?;

    let psm = PSM_DYN_START + 5;
    println!("Listening on PSM {}. Press enter to quit.", psm);

    let local_sa = SocketAddr { addr: adapter_addr, addr_type: adapter_addr_type, psm };
    let listener = StreamListener::bind(local_sa).await?;

    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();

    loop {
        println!("Waiting for connection...");

        let (stream, sa) = tokio::select! {
            l = listener.accept() => {
                match l {
                    Ok(v) => v,
                    Err(err) => {
                        println!("Accepting connection failed: {}", &err);
                        continue;
                    }}
            },
            _ = lines.next_line() => break,
        };

        println!("Accepted connection from {:?}", &sa);
        sleep(Duration::from_secs(5)).await;
    }

    println!("Removing advertisement");
    drop(adv_handle);
    sleep(Duration::from_secs(1)).await;

    Ok(())
}
