//! Serves a Bluetooth GATT echo server.

use bluer::{
    adv::Advertisement,
    gatt::{
        local::{
            characteristic_control, Application, Characteristic, CharacteristicControlEvent,
            CharacteristicNotify, CharacteristicNotifyMethod, CharacteristicWrite, CharacteristicWriteMethod,
            Service,
        },
        CharacteristicReader, CharacteristicWriter,
    },
};
use futures::{future, pin_mut, StreamExt};
use std::time::Duration;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    sync::mpsc,
    time::sleep,
};

include!("gatt_echo.inc");

enum WriterMsg {
    SetWriter(CharacteristicWriter),
    ClearWriter,
    Data(Vec<u8>),
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> bluer::Result<()> {
    env_logger::init();
    let session = bluer::Session::new().await?;
    let adapter = session.default_adapter().await?;
    adapter.set_powered(true).await?;

    println!("Advertising on Bluetooth adapter {} with address {}", adapter.name(), adapter.address().await?);
    let le_advertisement = Advertisement {
        service_uuids: vec![SERVICE_UUID].into_iter().collect(),
        discoverable: Some(true),
        local_name: Some("gatt_echo_server".to_string()),
        ..Default::default()
    };
    let adv_handle = adapter.advertise(le_advertisement).await?;

    println!("Serving GATT echo service on Bluetooth adapter {}", adapter.name());
    let (char_control, char_handle) = characteristic_control();
    let app = Application {
        services: vec![Service {
            uuid: SERVICE_UUID,
            primary: true,
            characteristics: vec![Characteristic {
                uuid: CHARACTERISTIC_UUID,
                write: Some(CharacteristicWrite {
                    write_without_response: true,
                    method: CharacteristicWriteMethod::Io,
                    ..Default::default()
                }),
                notify: Some(CharacteristicNotify {
                    notify: true,
                    method: CharacteristicNotifyMethod::Io,
                    ..Default::default()
                }),
                control_handle: char_handle,
                ..Default::default()
            }],
            ..Default::default()
        }],
        ..Default::default()
    };
    let app_handle = adapter.serve_gatt_application(app).await?;

    println!("Echo service ready. Press enter to quit.");
    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();

    let mut read_buf = Vec::new();
    let mut reader_opt: Option<CharacteristicReader> = None;
    let mut has_writer = false;
    pin_mut!(char_control);

    let (tx, mut rx) = mpsc::channel(50);
    tokio::spawn(async move {
        let mut writer_opt: Option<CharacteristicWriter> = None;
        while let Some(msg) = rx.recv().await {
            match msg {
                WriterMsg::SetWriter(w) => writer_opt = Some(w),
                WriterMsg::ClearWriter => {
                    if writer_opt.is_some() {
                        println!("Dropping writer");
                        writer_opt = None;
                    }
                }
                WriterMsg::Data(data) => {
                    if let Some(writer) = &mut writer_opt {
                        if let Err(err) = writer.write_all(&data).await {
                            println!("Write failed: {}", &err);
                            // Stop processing messages to signal failure to the sender
                            break;
                        }
                    }
                }
            }
        }
    });

    loop {
        tokio::select! {
            _ = lines.next_line() => break,
            evt = char_control.next() => {
                match evt {
                    Some(CharacteristicControlEvent::Write(req)) => {
                        println!("Accepting write request event with MTU {}", req.mtu());
                        read_buf = vec![0; req.mtu()];
                        reader_opt = Some(req.accept()?);
                    },
                    Some(CharacteristicControlEvent::Notify(notifier)) => {
                        println!("Accepting notify request event with MTU {}", notifier.mtu());
                        has_writer = true;
                        let _ = tx.send(WriterMsg::SetWriter(notifier)).await;
                    },
                    None => break,
                }
            },
            read_res = async {
                match &mut reader_opt {
                    Some(reader) if has_writer => reader.read(&mut read_buf).await,
                    _ => future::pending().await,
                }
            } => {
                match read_res {
                    Ok(0) => {
                        println!("Read stream ended");
                        reader_opt = None;
                        if has_writer {
                            has_writer = false;
                            let _ = tx.send(WriterMsg::ClearWriter).await;
                        }
                    }
                    Ok(n) => {
                        let data = read_buf[..n].to_vec();
                        if let Err(e) = tx.send(WriterMsg::Data(data)).await {
                            eprintln!("Error sending to writer: {}", e);
                            break;
                        }
                    }
                    Err(err) => {
                        println!("Read stream error: {}", &err);
                        reader_opt = None;
                    }
                }
            }
        }
    }

    println!("Removing service and advertisement");
    drop(app_handle);
    drop(adv_handle);
    sleep(Duration::from_secs(1)).await;

    Ok(())
}
