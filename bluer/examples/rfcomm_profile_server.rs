//! Server using Profile API.

use bluer::{
    agent::Agent,
    rfcomm::{Profile, Role, Channel},
    Session, Uuid,
};
use futures::StreamExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

include!("rfcomm.inc");

#[tokio::main]
async fn main() -> bluer::Result<()> {
    env_logger::init();
    let session = Session::new().await?;
    let adapter = session.default_adapter().await?;
    adapter.set_powered(true).await?;
    adapter.set_discoverable(true).await?;
    adapter.set_discoverable_timeout(0).await?;
    adapter.set_pairable(false).await?;
    let adapter_addr = adapter.address().await?;

    let agent = Agent::default();
    let _agent_hndl = session.register_agent(agent).await?;

    let uuid = PROFILE_UUID;
    
    println!("Registering profile with UUID {}...", uuid);

    let profile = Profile {
        uuid,
        name: Some("Test Profile".to_string()),
        role: Some(Role::Server),
        channel: Channel::Auto,
        require_authentication: Some(false),
        require_authorization: Some(false),
        auto_connect: Some(true),
        ..Default::default()
    };

    let mut handle = session.register_profile(profile).await?;

    println!("Listening on {}. Press enter to quit.", adapter_addr);

    loop {
        tokio::select! {
            req_opt = handle.next() => {
                if let Some(req) = req_opt {
                    println!("Accepted connection from {}", req.device());
                    let mut stream = req.accept()?;
                    println!("Sending hello");
                    if let Err(err) = stream.write_all(HELLO_MSG).await {
                        println!("Write failed: {}", &err);
                        continue;
                    }
                    
                    // Echo loop
                    let mut buf = [0u8; 1024];
                    loop {
                        let n = match stream.read(&mut buf).await {
                            Ok(0) => break,
                            Ok(n) => n,
                            Err(_) => break,
                        };
                        if stream.write_all(&buf[..n]).await.is_err() {
                            break;
                        }
                    }
                    println!("Connection closed");
                } else {
                    break;
                }
            }
            _ = tokio::signal::ctrl_c() => {
                break;
            }
        }
    }

    Ok(())
}
