#![feature(generic_associated_types)]
//! Attach and send/receive BT Mesh messages
//!
//! Example meshd
//! [bluer/bluer]$ sudo /usr/libexec/bluetooth/bluetooth-meshd --config ${PWD}/meshd/config --storage ${PWD}/meshd/lib --debug
//!
//! Example receive
//! [bluer/bluer]$ RUST_LOG=TRACE cargo +nightly run --example mesh_sensor_client -- --token 7eb48c91911361da
//!
//! Example send
//! [bluer/bluer]$ RUST_LOG=TRACE cargo +nightly run --example mesh_sensor_server -- --token dae519a06e504bd3

use bluer::mesh::{
    application::{Application, ApplicationMessage},
    element::*,
};
use btmesh_common::{opcode::Opcode, CompanyIdentifier, ParseError};
use btmesh_models::{
    sensor::{PropertyId, SensorClient, SensorConfig, SensorData, SensorDescriptor, SensorMessage},
    Message, Model,
};
use clap::Parser;
use dbus::Path;
use futures::{pin_mut, StreamExt};
use std::{sync::Arc, time::Duration};
use tokio::{signal, sync::mpsc, time::sleep};
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
struct Args {
    #[clap(short, long)]
    token: Option<String>,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    let args = Args::parse();
    let session = bluer::Session::new().await?;

    let mesh = session.mesh().await?;

    let (element_control, element_handle) = element_control(5);
    let (app_tx, app_rx) = mpsc::channel(1);

    let root_path = Path::from("/mesh_client");
    let app_path = Path::from(format!("{}/{}", root_path.clone(), "application"));
    let element_path = Path::from(format!("{}/{}", root_path.clone(), "ele00"));

    let sim = Application {
        path: app_path,
        elements: vec![Element {
            path: element_path,
            location: None,
            models: vec![Arc::new(FromDrogue::new(SensorClient::<SensorModel, 1, 1>::new()))],
            control_handle: Some(element_handle),
        }],
        events_tx: app_tx,
        provisioner: None,
        agent: Default::default(),
        properties: Default::default(),
    };

    let registered = mesh.application(root_path.clone(), sim).await?;

    match args.token {
        Some(token) => {
            println!("Attaching with {}", token);
            let _node = mesh.attach(root_path.clone(), &token).await?;
        }
        None => {
            let device_id = Uuid::new_v4();
            println!("Joining device: {}", device_id.as_simple());

            mesh.join(root_path.clone(), device_id).await?;
        }
    }

    println!("Sensor client ready. Press Ctrl+C to quit.");
    pin_mut!(element_control);
    let mut app_stream = ReceiverStream::new(app_rx);

    loop {
        tokio::select! {
            _ = signal::ctrl_c() => break,
            evt = element_control.next() => {
                match evt {
                    Some(msg) => {
                        match msg {
                            ElementMessage::Received(received) => {
                                match SensorClient::<SensorModel, 1, 1>::parse(&received.opcode, &received.parameters).map_err(|_| std::fmt::Error)? {
                                    Some(message) => {
                                        match message {
                                            SensorMessage::Status(status) => {
                                            println!("Received {:?}", status.data);
                                        },
                                        _ => todo!(),
                                    }
                                },
                                None => todo!()
                            }
                        },
                        ElementMessage::DevKey(_) => {
                            todo!()
                        }
                    }
                    },
                    None => break,
                }
            },
            app_evt = app_stream.next() => {
                match app_evt {
                    Some(msg) => {
                        match msg {
                            ApplicationMessage::JoinComplete(token) => {
                                println!("Joined with token {:016x}", token);
                                println!("Attaching");
                                let _node = mesh.attach(root_path.clone(), &format!("{:016x}", token)).await?;
                            },
                            ApplicationMessage::JoinFailed(reason) => {
                                println!("Failed to join: {}", reason);
                                break;
                            },
                        }
                    },
                    None => break,
                }
            }
        }
    }

    drop(registered);
    sleep(Duration::from_secs(1)).await;

    Ok(())
}

#[derive(Clone, Debug)]
pub struct SensorModel;

#[derive(Clone, Debug, Default)]
pub struct Temperature(f32);

impl SensorConfig for SensorModel {
    type Data = Temperature;

    const DESCRIPTORS: &'static [SensorDescriptor] = &[SensorDescriptor::new(PropertyId(0x4F), 1)];
}

impl SensorData for Temperature {
    fn decode(&mut self, id: PropertyId, params: &[u8]) -> Result<(), ParseError> {
        if id.0 == 0x4F {
            self.0 = params[0] as f32 / 2.0;
            Ok(())
        } else {
            Err(ParseError::InvalidValue)
        }
    }

    fn encode<const N: usize>(
        &self, _: PropertyId, xmit: &mut heapless::Vec<u8, N>,
    ) -> Result<(), InsufficientBuffer> {
        xmit.extend_from_slice(&self.0.to_le_bytes()).map_err(|_| InsufficientBuffer)?;
        Ok(())
    }
}

const COMPANY_IDENTIFIER: CompanyIdentifier = CompanyIdentifier(0x05F1);
const COMPANY_MODEL: ModelIdentifier = ModelIdentifier::Vendor(COMPANY_IDENTIFIER, 0x0001);

#[derive(Clone, Debug)]
pub struct VendorModel;

impl Model for VendorModel {
    const IDENTIFIER: ModelIdentifier = COMPANY_MODEL;
    type Message = VendorMessage;

    fn parse(_opcode: &Opcode, _parameters: &[u8]) -> Result<Option<Self::Message>, ParseError> {
        unimplemented!();
    }
}

#[derive(Copy, Clone)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum VendorMessage {}

impl Message for VendorMessage {
    fn opcode(&self) -> Opcode {
        unimplemented!();
    }

    fn emit_parameters<const N: usize>(
        &self, _xmit: &mut heapless::Vec<u8, N>,
    ) -> Result<(), InsufficientBuffer> {
        unimplemented!();
    }
}