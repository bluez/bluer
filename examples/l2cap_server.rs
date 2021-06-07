//! Opens a listening L2CAP socket and accepts connections.

use blez::{
    adv::Advertisement,
    l2cap::{Listener, PSM_DYN_START},
};
use std::time::Duration;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    time::sleep,
};

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
        discoverable: Some(true),
        local_name: Some("l2cap_server".to_string()),
        ..Default::default()
    };
    let adv_handle = adapter.advertise(le_advertisement).await?;

    let psm = PSM_DYN_START + 5;
    println!("Listening on PSM {}", psm);
    let listener = Listener::bind(adapter_addr, adapter_addr_type, psm).await?;

    loop {
        println!("Waiting for connection...");

        let (stream, addr, addr_type) = match listener.accept().await {
            Ok(v) => v,
            Err(err) => {
                println!("Accepting connection failed: {}", &err);
                continue;
            }
        };

        println!("Accepted connection from {} address {}", &addr_type, &addr);
        sleep(Duration::from_secs(10)).await;
    }

    //Ok(())
}
