#![feature(generic_associated_types)]
//! Attach and send/receive BT Mesh messages
//!
//! Example meshd
//! [bluer/bluer]$ sudo /usr/libexec/bluetooth/bluetooth-meshd --config ${PWD}/meshd/config --storage ${PWD}/meshd/lib --debug
//!
//! To demo device join, run client or server without a token
//! [bluer/bluer]$ RUST_LOG=TRACE cargo +nightly run --features=mesh --example mesh_sensor_client
//!
//! Example provisioner
//! [bluer/bluer]$ RUST_LOG=TRACE cargo +nightly run --features=mesh --example mesh_provisioner -- --token 84783e12f11c4dcd --uuid 4bd9876a3e4844bbb4339ef42f614f1f

use bluer::{
    mesh::{
        application::Application,
        element::*,
        provisioner::{Provisioner, ProvisionerControlHandle, ProvisionerMessage},
    },
    Uuid,
};
use btmesh_models::{
    foundation::configuration::{
        app_key::AppKeyMessage, ConfigurationClient, ConfigurationMessage, ConfigurationServer,
    },
    Message, Model,
};
use clap::Parser;
use futures::{pin_mut, StreamExt};
use std::{sync::Arc, time::Duration};
use tokio::{signal, sync::mpsc, time::sleep};
use tokio_stream::wrappers::ReceiverStream;

#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
struct Args {
    #[clap(short, long)]
    token: String,
    #[clap(short, long)]
    uuid: String,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    let args = Args::parse();
    let session = bluer::Session::new().await?;

    let mesh = session.mesh().await?;

    let (element_control, element_handle) = element_control(5);
    let (app_tx, _app_rx) = mpsc::channel(1);

    let (prov_tx, prov_rx) = mpsc::channel(1);

    let sim = Application {
        elements: vec![Element {
            location: None,
            models: vec![
                Arc::new(FromDrogue::new(ConfigurationServer::default())),
                Arc::new(FromDrogue::new(ConfigurationClient::default())),
            ],
            control_handle: Some(element_handle),
        }],
        provisioner: Some(Provisioner {
            control_handle: ProvisionerControlHandle { messages_tx: prov_tx },
            start_address: 0xbd,
        }),
        events_tx: app_tx,
        agent: Default::default(),
        properties: Default::default(),
    };

    let (registered, node) = mesh.attach(sim.clone(), &args.token).await?;

    node.management.add_node(Uuid::parse_str(&args.uuid)?).await?;

    let mut prov_stream = ReceiverStream::new(prov_rx);
    pin_mut!(element_control);

    loop {
        tokio::select! {
            _ = signal::ctrl_c() => break,
            evt = prov_stream.next() => {
                match evt {
                    Some(msg) => {
                        match msg {
                            ProvisionerMessage::AddNodeComplete(uuid, unicast, count) => {
                                println!("Successfully added node {:?} to the address {:#04x} with {:?} elements", uuid, unicast, count);

                                sleep(Duration::from_secs(1)).await;
                                node.add_app_key(registered.elements[0].clone(), unicast, 0, 0, false).await?;

                                // example composition get
                                // let message = ConfigurationMessage::CompositionData(CompositionDataMessage::Get(0));
                                // node.dev_key_send::<ConfigurationServer>(message, element_path.clone(), unicast, true, 0 as u16).await?;

                                // example bind
                                // let payload = ModelAppPayload {
                                //     element_address: unicast.try_into().map_err(|_| ReqError::Failed)?,
                                //     app_key_index: AppKeyIndex::new(0),
                                //     model_identifier: SENSOR_SERVER,
                                // };

                                // let message = ConfigurationMessage::from(ModelAppMessage::Bind(payload));
                                // node.dev_key_send::<ConfigurationServer>(message, element_path.clone(), unicast, true, 0 as u16).await?;
                            },
                            ProvisionerMessage::AddNodeFailed(uuid, reason) => {
                                println!("Failed to add node {:?}: '{:?}'", uuid, reason);
                                break;
                            }
                        }
                    },
                    None => break,
                }
            },
            evt = element_control.next() => {
                match evt {
                    Some(msg) => {
                        match msg {
                            ElementMessage::Received(received) => {
                                println!("Received element message: {:?}", received);
                            },
                            ElementMessage::DevKey(received) => {
                                println!("Received dev key message: {:?}", received);
                                match ConfigurationServer::parse(&received.opcode, &received.parameters).map_err(|_| std::fmt::Error)? {
                                    Some(message) => {
                                        match message {
                                            ConfigurationMessage::AppKey(key) => {
                                                match key {
                                                    AppKeyMessage::Status(status) => {
                                                        println!("Received keys {:?} {:?}", status.indexes.net_key(), status.indexes.app_key())
                                                    },
                                                    _ => println!("Received key message {:?}", key.opcode()),
                                                }
                                                break;
                                            },
                                            _ => {
                                                println!("Received dev key message {:?}", message.opcode());
                                                break;
                                            }
                                        }
                                    },
                                    None => break,
                                }
                            }
                    }
                    },
                    None => break,
                }
            },
        }
    }

    // Example agent function
    // pub fn display_numeric(req: DisplayNumeric) -> ReqResult<()> {
    //     println!("Enter '{:?}' on the remote device!", req.number);
    //     Ok(())
    // }

    println!("Shutting down");
    drop(registered);
    sleep(Duration::from_secs(1)).await;

    Ok(())
}
