//! Simple Bluetooth Agent example.
//!
//! This example registers a "KeyboardDisplay" agent and prints events.

use bluer::agent::{Agent, ReqResult};
use std::time::Duration;
use tokio::time::sleep;

#[tokio::main(flavor = "current_thread")]
async fn main() -> bluer::Result<()> {
    env_logger::init();
    let session = bluer::Session::new().await?;

    let agent = Agent {
        request_pin_code: Some(Box::new(|req| {
            Box::pin(async move {
                println!("RequestPinCode: {:?}", req);
                ReqResult::Ok("1234".to_string())
            })
        })),
        display_pin_code: Some(Box::new(|req| {
            Box::pin(async move {
                println!("DisplayPinCode: {:?}", req);
                ReqResult::Ok(())
            })
        })),
        request_passkey: Some(Box::new(|req| {
            Box::pin(async move {
                println!("RequestPasskey: {:?}", req);
                ReqResult::Ok(123456)
            })
        })),
        display_passkey: Some(Box::new(|req| {
            Box::pin(async move {
                println!("DisplayPasskey: {:?}", req);
                ReqResult::Ok(())
            })
        })),
        request_confirmation: Some(Box::new(|req| {
            Box::pin(async move {
                println!("RequestConfirmation: {:?}", req);
                ReqResult::Ok(())
            })
        })),
        request_authorization: Some(Box::new(|req| {
            Box::pin(async move {
                println!("RequestAuthorization: {:?}", req);
                ReqResult::Ok(())
            })
        })),
        authorize_service: Some(Box::new(|req| {
            Box::pin(async move {
                println!("AuthorizeService: {:?}", req);
                ReqResult::Ok(())
            })
        })),
        ..Default::default()
    };

    let _handle = session.register_agent(agent).await?;

    println!("Agent registered. Press enter to quit.");
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;

    println!("Dropping handle...");
    drop(_handle);
    sleep(Duration::from_secs(1)).await;

    Ok(())
}
