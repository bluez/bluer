//! Serves a Bluetooth GATT application.
use std::time::Duration;

use blurz::{gatt, LeAdvertisement};
use futures::FutureExt;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    time::sleep,
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> blurz::Result<()> {
    let service_uuid = "9643735b-c62e-4717-0000-61abaf5abc8e".parse().unwrap();
    let characteristic_uuid = "9643735b-c62e-4717-0001-61abaf5abc8e".parse().unwrap();

    let session = blurz::Session::new().await?;
    let adapter_names = session.adapter_names().await?;
    let adapter_name = adapter_names.first().expect("No Bluetooth adapter present");
    let adapter = session.adapter(&adapter_name)?;

    println!("Advertising on Bluetooth adapter {}: {}", &adapter_name, adapter.address().await?);

    let le_advertisement = LeAdvertisement {
        service_uuids: vec![service_uuid].into_iter().collect(),
        discoverable: Some(true),
        local_name: Some("gatt_server".to_string()),
        ..Default::default()
    };
    let adv_handle = adapter.le_advertise(le_advertisement).await?;

    println!("Serving GATT application on Bluetooth adapter {}", &adapter_name);

    let mut flags = gatt::CharacteristicFlags::default();
    flags.read = true;
    flags.write_without_response = true;
    flags.notify = true;
    //flags.notify = true;

    let app = gatt::local::Application {
        services: vec![gatt::local::Service {
            uuid: service_uuid,
            primary: true,
            characteristics: vec![gatt::local::Characteristic {
                uuid: characteristic_uuid,
                flags: flags,
                descriptors: vec![],
                read_value: Some(Box::new(|req| {
                    async move {
                        println!("Read request: {:?}", &req);
                        Ok(vec![1, 2, 3])
                    }
                    .boxed()
                })),
                write_value: Some(Box::new(|value, req| {
                    async move {
                        println!("Write request {:?} with value {:?}", &req, &value);
                        Ok(())
                    }
                    .boxed()
                })),
            }],
        }],
    };
    let app_handle = adapter.serve_gatt_application(app).await?;

    println!("Press enter to quit");
    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();
    let _ = lines.next_line().await;

    println!("Removing application");
    drop(app_handle);
    drop(adv_handle);

    sleep(Duration::from_secs(1)).await;

    Ok(())
}
