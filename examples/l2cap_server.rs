//! Opens a listening L2CAP socket, accepts connections and echos incoming data.

use blez::{
    adv::Advertisement,
    l2cap::{SocketAddr, StreamListener, PSM_DYN_START},
};
use std::time::Duration;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    time::sleep,
};

const SERVICE_UUID: uuid::Uuid = uuid::Uuid::from_u128(0xFEED0000F00D);

include!("l2cap.inc");

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

    let local_sa = SocketAddr { addr: adapter_addr, addr_type: adapter_addr_type, psm: PSM };
    let listener = StreamListener::bind(local_sa).await?;

    println!("Listening on PSM {}. Press enter to quit.", listener.as_ref().local_addr()?.psm);
    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();

    loop {
        println!("\nWaiting for connection...");

        let (mut stream, sa) = tokio::select! {
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

        println!("Sending hello");
        if let Err(err) = stream.write_all(HELLO_MSG).await {
            println!("Write failed: {}", &err);
            continue;
        }

        loop {
            let mut buf = [0; 100];
            let n = match stream.read(&mut buf).await {
                Ok(0) => {
                    println!("Stream ended");
                    break;
                }
                Ok(n) => n,
                Err(err) => {
                    println!("Read failed: {}", &err);
                    continue;
                }
            };
            let buf = &buf[..n];

            println!("Echoing {} bytes", buf.len());
            if let Err(err) = stream.write_all(&buf).await {
                println!("Write failed: {}", &err);
                continue;
            }
        }
    }

    println!("Removing advertisement");
    drop(adv_handle);
    sleep(Duration::from_secs(1)).await;

    Ok(())
}
