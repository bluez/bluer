//! Serves a Bluetooth GATT application using the callback programming model.

use bluer::{
    adv::Advertisement,
    gatt::local::{
        Application, Characteristic, CharacteristicNotify, CharacteristicNotifyMethod, CharacteristicRead,
        CharacteristicWrite, CharacteristicWriteMethod, Service,
    },
};
use futures::FutureExt;
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    sync::Mutex,
    time::sleep,
};

include!("gatt.inc");

#[tokio::main(flavor = "current_thread")]
async fn main() -> bluer::Result<()> {
    env_logger::init();
    let session = bluer::Session::new().await?;
    let adapter_names = session.adapter_names().await?;
    let adapter_name = adapter_names.first().expect("No Bluetooth adapter present");
    let adapter = session.adapter(adapter_name)?;
    adapter.set_powered(true).await?;

    println!("Advertising on Bluetooth adapter {} with address {}", &adapter_name, adapter.address().await?);
    let mut manufacturer_data = BTreeMap::new();
    manufacturer_data.insert(MANUFACTURER_ID, vec![0x21, 0x22, 0x23, 0x24]);
    let le_advertisement = Advertisement {
        service_uuids: vec![SERVICE_UUID].into_iter().collect(),
        manufacturer_data,
        discoverable: Some(true),
        local_name: Some("gatt_server".to_string()),
        ..Default::default()
    };
    let adv_handle = adapter.advertise(le_advertisement).await?;

    println!("Serving GATT service on Bluetooth adapter {}", &adapter_name);
    let value = Arc::new(Mutex::new(vec![0x10, 0x01, 0x01, 0x10]));
    let value_read = value.clone();
    let value_write = value.clone();
    let value_notify = value.clone();
    let app = Application {
        services: vec![Service {
            uuid: SERVICE_UUID,
            primary: true,
            characteristics: vec![Characteristic {
                uuid: CHARACTERISTIC_UUID,
                read: Some(CharacteristicRead {
                    read: true,
                    fun: Box::new(move |req| {
                        let value = value_read.clone();
                        async move {
                            let value = value.lock().await.clone();
                            println!("Read request {:?} with value {:x?}", &req, &value);
                            Ok(value)
                        }
                        .boxed()
                    }),
                    ..Default::default()
                }),
                write: Some(CharacteristicWrite {
                    write: true,
                    write_without_response: true,
                    method: CharacteristicWriteMethod::Fun(Box::new(move |new_value, req| {
                        let value = value_write.clone();
                        async move {
                            println!("Write request {:?} with value {:x?}", &req, &new_value);
                            let mut value = value.lock().await;
                            *value = new_value;
                            Ok(())
                        }
                        .boxed()
                    })),
                    ..Default::default()
                }),
                notify: Some(CharacteristicNotify {
                    notify: true,
                    method: CharacteristicNotifyMethod::Fun(Box::new(move |mut notifier| {
                        let value = value_notify.clone();
                        async move {
                            tokio::spawn(async move {
                                println!(
                                    "Notification session start with confirming={:?}",
                                    notifier.confirming()
                                );
                                loop {
                                    {
                                        let mut value = value.lock().await;
                                        println!("Notifying with value {:x?}", &*value);
                                        if let Err(err) = notifier.notify(value.to_vec()).await {
                                            println!("Notification error: {}", &err);
                                            break;
                                        }
                                        println!("Decrementing each element by one");
                                        for v in &mut *value {
                                            *v = v.saturating_sub(1);
                                        }
                                    }
                                    sleep(Duration::from_secs(5)).await;
                                }
                                println!("Notification session stop");
                            });
                        }
                        .boxed()
                    })),
                    ..Default::default()
                }),
                ..Default::default()
            }],
            ..Default::default()
        }],
    };
    let app_handle = adapter.serve_gatt_application(app).await?;

    println!("Service ready. Press enter to quit.");
    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();
    let _ = lines.next_line().await;

    println!("Removing service and advertisement");
    drop(app_handle);
    drop(adv_handle);
    sleep(Duration::from_secs(1)).await;

    Ok(())
}
