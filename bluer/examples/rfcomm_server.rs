//! Opens a listening RFCOMM socket, accepts connections and echos incoming data.

use bluer::rfcomm::{Listener, SocketAddr};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

include!("rfcomm.inc");

#[tokio::main]
async fn main() -> bluer::Result<()> {
    env_logger::init();
    let session = bluer::Session::new().await?;
    let adapter = session.default_adapter().await?;
    adapter.set_powered(true).await?;
    adapter.set_discoverable(true).await?;
    let adapter_addr = adapter.address().await?;

    let local_sa = SocketAddr::new(adapter_addr, CHANNEL);
    let listener = Listener::bind(local_sa).await?;

    println!(
        "Listening on {} channel {}. Press enter to quit.",
        listener.as_ref().local_addr()?.addr,
        listener.as_ref().local_addr()?.channel
    );
    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();

    loop {
        println!("\nWaiting for connection...");

        let (mut stream, sa) = tokio::select! {
            l = listener.accept() => {
                match l {
                    Ok(v) => v,
                    Err(err) => {
                        println!("Accepting connection failed: {}", &err);
                        continue;
                    }}
            },
            _ = lines.next_line() => break,
        };

        println!("Accepted connection from {:?}", &sa);

        println!("Sending hello");
        if let Err(err) = stream.write_all(HELLO_MSG).await {
            println!("Write failed: {}", &err);
            continue;
        }

        loop {
            let buf_size = 1024;
            let mut buf = vec![0; buf_size as _];

            let n = match stream.read(&mut buf).await {
                Ok(0) => {
                    println!("Stream ended");
                    break;
                }
                Ok(n) => n,
                Err(err) => {
                    println!("Read failed: {}", &err);
                    break;
                }
            };
            let buf = &buf[..n];

            println!("Echoing {} bytes", buf.len());
            if let Err(err) = stream.write_all(buf).await {
                println!("Write failed: {}", &err);
                continue;
            }
        }
    }

    Ok(())
}
