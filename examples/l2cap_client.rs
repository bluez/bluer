//! Connects to l2cap_server and sends and receives test data.

use blez::{
    l2cap::{SocketAddr, Stream, PSM_DYN_START},
    Address, AddressType,
};
use rand::prelude::*;
use std::{env, process::exit};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

include!("l2cap.inc");

#[tokio::main(flavor = "current_thread")]
async fn main() -> blez::Result<()> {
    env_logger::init();

    let args: Vec<_> = env::args().collect();
    if args.len() != 2 {
        eprintln!("Specify target Bluetooth address as argment");
        exit(1);
    }

    let target_addr: Address = args[1].parse().expect("invalid address");
    let target_sa = SocketAddr::new(target_addr, AddressType::Public, PSM);

    println!("Connecting to {:?}", &target_sa);
    let mut stream = Stream::connect(target_sa).await.expect("connection failed");
    println!("Local address: {:?}", stream.as_ref().local_addr()?);
    println!("Remote address: {:?}", stream.peer_addr()?);
    println!("Send MTU: {}", stream.as_ref().send_mtu()?);
    println!("Recv MTU: {}", stream.as_ref().recv_mtu()?);
    println!("Security: {:?}", stream.as_ref().security()?);
    // println!("Flow control: {:?}", stream.as_ref().flow_control()?);

    println!("\nReceiving hello");
    let mut hello_buf = [0u8; HELLO_MSG.len()];
    stream.read_exact(&mut hello_buf).await.expect("read failed");
    println!("Received: {}", String::from_utf8_lossy(&hello_buf));
    if hello_buf != HELLO_MSG {
        panic!("Wrong hello message");
    }

    let mut rng = rand::thread_rng();
    for i in 0..15 {
        let len = rng.gen_range(0..1000);
        let data: Vec<u8> = (0..len).map(|_| rng.gen()).collect();

        println!("\nTest iteration {} with data size {}", i, len);
        stream.write_all(&data).await.expect("write failed");

        println!("Waiting for echo");
        let mut echo_buf = vec![0u8; len];
        stream.read_exact(&mut echo_buf).await.expect("read failed");

        if echo_buf != data {
            panic!("Echoed data does not match sent data");
        }
        println!("Data matches");
    }

    println!("Done");
    Ok(())
}
