//! Client using Profile API.

use bluer::{
    AdapterEvent, Address, Session,
    agent::Agent,
    rfcomm::{Profile, ReqError, Role},
};
use futures::StreamExt;
use std::{env, process::exit, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    time::sleep,
};

include!("rfcomm.inc");

#[tokio::main]
async fn main() -> bluer::Result<()> {
    env_logger::init();
    let session = Session::new().await?;
    let adapter = session.default_adapter().await?;
    adapter.set_powered(true).await?;
    adapter.set_pairable(false).await?;

    let args: Vec<_> = env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: rfcomm_profile_client <target_address>");
        exit(1);
    }

    let target_addr: Address = args[1].parse().expect("invalid address");
    let uuid = PROFILE_UUID;

    let agent = Agent::default();
    let _agent_hndl = session.register_agent(agent).await?;

    println!("Registering client profile...");
    let profile = Profile {
        uuid,
        name: Some("Test Client Profile".to_string()),
        role: Some(Role::Client),
        require_authentication: Some(false),
        require_authorization: Some(false),
        auto_connect: Some(true),
        ..Default::default()
    };

    let mut handle = session.register_profile(profile).await?;

    println!("Discovering device {}...", target_addr);
    let mut devs = adapter.discover_devices().await?;
    // Wait for device to be found
    while let Some(evt) = devs.next().await {
        if let AdapterEvent::DeviceAdded(addr) = evt
            && addr == target_addr
        {
            break;
        }
    }
    let dev = adapter.device(target_addr)?;
    drop(devs);

    println!("Connecting to device...");
    // Trigger connection
    loop {
        tokio::select! {
            res = async {
                let _ = dev.connect().await;
                dev.connect_profile(&uuid).await
            } => {
                if let Err(err) = res {
                    println!("Connect profile failed: {err}, retrying...");
                }
                sleep(Duration::from_secs(3)).await;
            },
            req_opt = handle.next() => {
                if let Some(req) = req_opt {
                    println!("Connect request from {}", req.device());
                    if req.device() == target_addr {
                        println!("Accepting request...");
                        let mut stream = req.accept()?;

                        println!("\nReceiving hello");
                        let mut hello_buf = [0u8; HELLO_MSG.len()];
                        stream.read_exact(&mut hello_buf).await.expect("read failed");
                        println!("Received: {}", String::from_utf8_lossy(&hello_buf));
                        if hello_buf != HELLO_MSG {
                            panic!("Wrong hello message");
                        }

                        println!("Sending hello back");
                        stream.write_all(HELLO_MSG).await.expect("write failed");

                        println!("Test finished successfully");
                        return Ok(());
                    } else {
                        println!("Rejecting request from unknown device");
                        req.reject(ReqError::Rejected);
                    }
                } else {
                    break;
                }
            }
        }
    }

    Ok(())
}
