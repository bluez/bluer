//! Opens a listening L2CAP socket, accepts connections and echos incoming data.

use bluer::{
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

#[tokio::main]
async fn main() -> bluer::Result<()> {
    env_logger::init();
    let session = bluer::Session::new().await?;
    let adapter_names = session.adapter_names().await?;
    let adapter_name = adapter_names.first().expect("No Bluetooth adapter present");
    let adapter = session.adapter(adapter_name)?;
    adapter.set_powered(true).await?;
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

    let local_sa = SocketAddr::new(adapter_addr, adapter_addr_type, PSM);
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
        let recv_mtu = stream.as_ref().recv_mtu()?;

        println!("Accepted connection from {:?} with receive MTU {} bytes", &sa, &recv_mtu);

        println!("Sending hello");
        if let Err(err) = stream.write_all(HELLO_MSG).await {
            println!("Write failed: {}", &err);
            continue;
        }

        let mut n = 0;
        loop {
            n += 1;

            // Vary buffer size between MTU and smaller value to test
            // partial reads.
            let buf_size = if n % 5 == 0 { recv_mtu - 70 } else { recv_mtu };
            let mut buf = vec![0; buf_size as _];

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
            if let Err(err) = stream.write_all(buf).await {
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
