//! Connects to a Bluetooth GATT application.

use std::time::Duration;

use blurz::{gatt::remote::Characteristic, Device, DeviceEvent, Result};
use futures::{pin_mut, StreamExt};
use tokio::time::sleep;
use uuid::Uuid;

async fn find_our_characteristic(device: &Device) -> Result<Option<Characteristic>> {
    let service_uuid: Uuid = "9643735b-c62e-4717-0000-61abaf5abc8e".parse().unwrap();
    let characteristic_uuid: Uuid = "9643735b-c62e-4717-0001-61abaf5abc8e".parse().unwrap();

    let addr = device.address();
    let uuids = device.uuids().await?.unwrap_or_default();
    println!("Discovered device {} with service UUIDs {:?}", addr, &uuids);

    if uuids.contains(&service_uuid) {
        println!("    Device provides our service!");

        sleep(Duration::from_secs(2)).await;
        if !device.is_connected().await? {
            println!("    Connecting...");
            let mut retries = 2;
            loop {
                match device.connect().await {
                    Ok(()) => break,
                    Err(err) if retries > 0 => {
                        println!("    Connect error: {}", &err);
                        retries -= 1;
                    }
                    Err(err) => return Err(err),
                }
            }
            println!("    Connected");
        } else {
            println!("    Already connected");
        }

        println!("    Waiting for service resolution...");
        device.wait_for_services_resolved().await?;
        println!("    Services resolved");

        for service in device.services().await? {
            let uuid = service.uuid().await?;
            println!("    Service UUID: {}", &uuid);
            if uuid == service_uuid {
                println!("    Found our service!");
                for char in service.characteristics().await? {
                    let uuid = char.uuid().await?;
                    println!("    Characteristic UUID: {}", &uuid);
                    if uuid == characteristic_uuid {
                        println!("    Found our characteristic!");
                        return Ok(Some(char));
                    }
                }
            }
        }

        println!("    Not found!");
    }

    Ok(None)
}

async fn exercise_characteristic(char: &Characteristic) -> Result<()> {
    println!("    Characteristic flags: {:?}", char.flags().await?);
    println!("    Reading characteristic value");
    let value = char.read().await?;
    println!("    Read value: {:?}", &value);

    let data = vec![10, 11, 12, 13];
    println!("    Writing characteristic value with data {:?}", &data);
    char.write(&data).await?;

    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> blurz::Result<()> {
    let session = blurz::Session::new().await?;
    let adapter_names = session.adapter_names().await?;
    let adapter_name = adapter_names.first().expect("No Bluetooth adapter present");
    let adapter = session.adapter(&adapter_name)?;

    println!("Scanning on Bluetooth adapter {}: {}", &adapter_name, adapter.address().await?);

    let device_events = adapter.discover_devices().await?;
    pin_mut!(device_events);

    while let Some(evt) = device_events.next().await {
        match evt {
            DeviceEvent::Added(addr) => {
                let device = adapter.device(addr)?;
                match find_our_characteristic(&device).await {
                    Ok(Some(char)) => {
                        if let Err(err) = exercise_characteristic(&char).await {
                            println!("    Characteristic exercise failed: {}", &err);
                        }
                    }
                    Ok(None) => (),
                    Err(err) => {
                        println!("    Device failed: {}", &err);
                        let _ = adapter.remove_device(device.address()).await;
                    }
                }
                let _ = device.disconnect().await;
            }
            DeviceEvent::Removed(addr) => {
                println!("Device removed {}", addr);
            }
        }
    }

    Ok(())
}
