//! Connects to our Bluetooth GATT service and exercises the characteristic.

use blez::{gatt::remote::Characteristic, AdapterEvent, Device, Result};
use futures::{pin_mut, StreamExt};
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    time::sleep,
};

include!("gatt.inc");

async fn find_our_characteristic(device: &Device) -> Result<Option<Characteristic>> {
    let addr = device.address();
    let uuids = device.uuids().await?.unwrap_or_default();
    println!("Discovered device {} with service UUIDs {:?}", addr, &uuids);
    let md = device.manufacturer_data().await?;
    println!("    Manufacturer data: {:x?}", &md);

    if uuids.contains(&SERVICE_UUID) {
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

        println!("    Enumerating services...");
        for service in device.services().await? {
            let uuid = service.uuid().await?;
            println!("    Service UUID: {}", &uuid);
            if uuid == SERVICE_UUID {
                println!("    Found our service!");
                for char in service.characteristics().await? {
                    let uuid = char.uuid().await?;
                    println!("    Characteristic UUID: {}", &uuid);
                    if uuid == CHARACTERISTIC_UUID {
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
    sleep(Duration::from_secs(1)).await;

    if char.flags().await?.read {
        println!("    Reading characteristic value");
        let value = char.read().await?;
        println!("    Read value: {:x?}", &value);
        sleep(Duration::from_secs(1)).await;
    }

    let data = vec![0xee, 0x11, 0x11, 0x0];
    println!("    Writing characteristic value {:x?} using function call", &data);
    char.write(&data).await?;
    let value = char.read().await?;
    println!("    Read value back: {:x?}", &value);
    sleep(Duration::from_secs(1)).await;

    println!("    Obtaining write IO");
    let mut write_io = char.write_io().await?;
    println!("    Write IO obtained");
    println!("    Writing characteristic value {:x?} five times using IO", &data);
    for _ in 0..5u8 {
        let written = write_io.write(&data).await?;
        println!("    {} bytes written", written);
    }
    println!("    Closing write IO");
    drop(write_io);
    sleep(Duration::from_secs(1)).await;

    println!("    Starting notification session");
    {
        let notify = char.notify().await?;
        pin_mut!(notify);
        for _ in 0..5u8 {
            match notify.next().await {
                Some(value) => {
                    println!("    Notification value: {:x?}", &value);
                }
                None => {
                    println!("    Notification session was terminated");
                }
            }
        }
        println!("    Stopping notification session");
    }
    sleep(Duration::from_secs(1)).await;

    println!("    Obtaining notification IO");
    let mut notify_io = char.notify_io().await?;
    println!("    Obtained notification IO with MTU={}", notify_io.mtu());
    for _ in 0..5u8 {
        let mut buf = vec![0; notify_io.mtu()];
        match notify_io.read(&mut buf).await {
            Ok(0) => {
                println!("    Notification IO end of stream");
                break;
            }
            Ok(read) => {
                println!("    Notified with {} bytes: {:x?}", read, &buf[..]);
            }
            Err(err) => {
                println!("    Notification IO failed: {}", &err);
                break;
            }
        }
    }
    println!("    Stopping notification IO");
    drop(notify_io);
    sleep(Duration::from_secs(1)).await;

    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> blez::Result<()> {
    env_logger::init();
    let session = blez::Session::new().await?;
    let adapter_names = session.adapter_names().await?;
    let adapter_name = adapter_names.first().expect("No Bluetooth adapter present");
    let adapter = session.adapter(&adapter_name)?;

    {
        println!(
            "Discovering on Bluetooth adapter {} with address {}\n",
            &adapter_name,
            adapter.address().await?
        );
        let discover = adapter.discover_devices().await?;
        pin_mut!(discover);
        let mut done = false;
        while let Some(evt) = discover.next().await {
            match evt {
                AdapterEvent::DeviceAdded(addr) => {
                    let device = adapter.device(addr)?;
                    match find_our_characteristic(&device).await {
                        Ok(Some(char)) => match exercise_characteristic(&char).await {
                            Ok(()) => {
                                println!("    Characteristic exercise completed");
                                done = true;
                            }
                            Err(err) => {
                                println!("    Characteristic exercise failed: {}", &err);
                            }
                        },
                        Ok(None) => (),
                        Err(err) => {
                            println!("    Device failed: {}", &err);
                            let _ = adapter.remove_device(device.address()).await;
                        }
                    }
                    match device.disconnect().await {
                        Ok(()) => println!("    Device disconnected"),
                        Err(err) => println!("    Device disconnection failed: {}", &err),
                    }
                    println!();
                }
                AdapterEvent::DeviceRemoved(addr) => {
                    println!("Device removed {}", addr);
                }
                _ => (),
            }
            if done {
                break;
            }
        }
        println!("Stopping discovery");
    }

    sleep(Duration::from_secs(1)).await;
    Ok(())
}
