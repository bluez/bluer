//! Arbitrary L2CAP connections and listens.

use bluer::{
    adv::{Advertisement, AdvertisementHandle},
    l2cap::{Socket, SocketAddr, Stream, StreamListener},
    Address, AddressType, Uuid,
};
use bytes::BytesMut;
use clap::Parser;
use crossterm::{terminal, tty::IsTty};
use futures::{future, pin_mut};
use libc::{STDIN_FILENO, STDOUT_FILENO};
use rand::prelude::*;
use std::{
    collections::VecDeque,
    ffi::OsString,
    process::{exit, Command, Stdio},
    time::{Duration, Instant},
};
use tab_pty_process::AsyncPtyMaster;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    select,
    time::sleep,
};
use tokio_compat_02::IoCompat;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const SERVICE_UUID: Uuid = Uuid::from_u128(0xdb9517c5d364d6fa1160931502091984);

#[derive(Parser)]
#[clap(
    name = "l2cat",
    about = "Arbitrary Bluetooth BR/EDR/LE L2CAP connections and listens.",
    author = "Sebastian Urban <surban@surban.net>",
    version = env!("CARGO_PKG_VERSION"),
)]
struct Opts {
    #[clap(subcommand)]
    cmd: Cmd,
}

#[derive(Parser)]
enum Cmd {
    /// Connect to remote device.
    Connect(ConnectOpts),
    /// Listen for connection from remote device.
    Listen(ListenOpts),
    /// Listen for connection from remote device and serve a program
    /// once a connection is established.
    Serve(ServeOpts),
    /// Speed test client.
    SpeedClient(SpeedClientOpts),
    /// Speed test server.
    SpeedServer(SpeedServerOpts),
}

#[derive(Parser)]
struct ConnectOpts {
    /// Address of local Bluetooth adapter to use.
    #[clap(long, short)]
    bind: Option<Address>,
    /// Switch the terminal into raw mode when input is a TTY.
    /// Use together with --pty when serving.
    #[clap(long, short)]
    raw: bool,
    /// Use classic Bluetooth (BR/EDR).
    /// Otherwise Bluetooth Low Energy (LE) is used.
    #[clap(long, short = 'c')]
    br_edr: bool,
    /// Public Bluetooth address of target device.
    address: Address,
    /// Target PSM.
    ///
    /// For BR/EDR, it must follow the bit pattern xxxxxxx0_xxxxxxx1.
    psm: u16,
}

impl ConnectOpts {
    pub async fn perform(self) -> Result<()> {
        let addr_type = if self.br_edr { AddressType::BrEdr } else { AddressType::LePublic };

        let socket = Socket::new_stream()?;
        let local_sa = match self.bind {
            Some(bind_addr) => SocketAddr::new(bind_addr, addr_type, 0),
            None if self.br_edr => SocketAddr::any_br_edr(),
            None => SocketAddr::any_le(),
        };
        socket.bind(local_sa)?;

        let peer_sa = SocketAddr::new(self.address, addr_type, self.psm);
        let stream = socket.connect(peer_sa).await?;

        let is_tty = std::io::stdin().is_tty();
        let in_raw = if is_tty && self.raw {
            terminal::enable_raw_mode()?;
            true
        } else {
            false
        };

        io_loop(stream, tokio::io::stdin(), tokio::io::stdout(), true, is_tty, true).await?;

        if in_raw {
            terminal::disable_raw_mode()?;
        }

        Ok(())
    }
}

#[derive(Parser)]
struct ListenOpts {
    /// Address of local Bluetooth adapter to use.
    #[clap(long, short)]
    bind: Option<Address>,
    /// Print listen and peer address to standard error.
    #[clap(long, short)]
    verbose: bool,
    /// Switch the terminal into raw mode when input is a TTY.
    #[clap(long)]
    raw: bool,
    /// Do not advertise device.
    #[clap(long, short)]
    no_advertise: bool,
    /// Use classic Bluetooth (BR/EDR).
    /// Otherwise Bluetooth Low Energy (LE) is used.
    #[clap(long, short = 'c')]
    br_edr: bool,
    /// PSM to listen on.
    ///
    /// For BR/EDR, it must follow the bit pattern xxxxxxx0_xxxxxxx1 and a
    /// value below 4097 is privileged.
    /// For LE, a value below 128 is privileged.
    /// Specify 0 to auto allocate an available PSM.
    psm: u16,
}

impl ListenOpts {
    pub async fn perform(self) -> Result<()> {
        let _adv = if !self.no_advertise { Some(advertise(self.br_edr).await?) } else { None };

        let address_type = if self.br_edr { AddressType::BrEdr } else { AddressType::LePublic };
        let local_sa = SocketAddr::new(self.bind.unwrap_or_else(Address::any), address_type, self.psm);
        let listen = StreamListener::bind(local_sa).await?;
        let local_sa = listen.as_ref().local_addr()?;
        if self.verbose && self.psm == 0 {
            eprintln!("Listening on PSM {}", local_sa.psm);
        }

        let (stream, peer_sa) = listen.accept().await?;
        if self.verbose {
            eprintln!("Connected from {}", peer_sa.addr);
        }

        let is_tty = std::io::stdin().is_tty();
        let in_raw = if is_tty && self.raw {
            terminal::enable_raw_mode()?;
            true
        } else {
            false
        };

        io_loop(stream, tokio::io::stdin(), tokio::io::stdout(), true, true, true).await?;

        if in_raw {
            terminal::disable_raw_mode()?;
        }

        Ok(())
    }
}

#[derive(Parser)]
struct ServeOpts {
    /// Address of local Bluetooth adapter to use.
    #[clap(long, short)]
    bind: Option<Address>,
    /// Print listen and peer address to standard error.
    #[clap(long, short)]
    verbose: bool,
    /// Do not advertise device.
    #[clap(long, short)]
    no_advertise: bool,
    /// Exit after handling one connection.
    #[clap(long, short)]
    one_shot: bool,
    /// Allocate a pseudo-terminal (PTY) for the program.
    /// Use together with --raw when connecting.
    #[clap(long, short)]
    pty: bool,
    /// Use classic Bluetooth (BR/EDR).
    /// Otherwise Bluetooth Low Energy (LE) is used.
    #[clap(long, short = 'c')]
    br_edr: bool,
    /// PSM to listen on.
    ///
    /// For BR/EDR, it must follow the bit pattern xxxxxxx0_xxxxxxx1 and a
    /// value below 4097 is privileged.
    /// For LE, a value below 128 is privileged.
    /// Specify 0 to auto allocate an available PSM.
    psm: u16,
    /// Program to execute once connection is established.
    command: OsString,
    /// Arguments to program.
    args: Vec<OsString>,
}

impl ServeOpts {
    pub async fn perform(self) -> Result<()> {
        use tab_pty_process::CommandExt;

        let _adv = if !self.no_advertise { Some(advertise(self.br_edr).await?) } else { None };

        let address_type = if self.br_edr { AddressType::BrEdr } else { AddressType::LePublic };
        let local_sa = SocketAddr::new(self.bind.unwrap_or_else(Address::any), address_type, self.psm);
        let listen = StreamListener::bind(local_sa).await?;
        let local_sa = listen.as_ref().local_addr()?;
        if !self.verbose && self.psm == 0 {
            eprintln!("Listening on PSM {}", local_sa.psm);
        }

        loop {
            let (stream, peer_sa) = listen.accept().await?;
            if self.verbose {
                eprintln!("Connected from {}", peer_sa.addr);
            }

            if self.pty {
                let ptymaster = AsyncPtyMaster::open()?;
                let mut cmd = Command::new(&self.command);
                cmd.args(&self.args);
                let child = match cmd.spawn_pty_async_raw(&ptymaster) {
                    Ok(child) => child,
                    Err(err) => {
                        eprintln!("Cannot execute {}: {}", &self.command.to_string_lossy(), &err);
                        continue;
                    }
                };

                let (pin, pout) = ptymaster.split();
                let pin = IoCompat::new(pin);
                let pout = IoCompat::new(pout);
                select! {
                    res = io_loop(stream, pin, pout, false, true, false) => {
                        res?;
                        if self.verbose {
                            eprintln!("Connection terminated");
                        }
                    },
                    _ = child => {
                        if self.verbose {
                            eprintln!("Process exited");
                        }
                    },
                }
            } else {
                let mut cmd = tokio::process::Command::new(&self.command);
                cmd.args(&self.args);
                cmd.kill_on_drop(true);
                cmd.stdin(Stdio::piped());
                cmd.stdout(Stdio::piped());
                let mut child = match cmd.spawn() {
                    Ok(child) => child,
                    Err(err) => {
                        eprintln!("Cannot execute {}: {}", &self.command.to_string_lossy(), &err);
                        continue;
                    }
                };

                let pin = child.stdout.take().unwrap();
                let pout = child.stdin.take().unwrap();
                select! {
                    res = io_loop(stream, pin, pout, false, true, false) => {
                        res?;
                        if self.verbose {
                            eprintln!("Connection terminated");
                        }
                    },
                    _ = child.wait() => {
                        if self.verbose {
                            eprintln!("Process exited");
                        }
                    },
                }
            }

            if self.one_shot {
                break;
            }
        }

        Ok(())
    }
}

async fn io_loop(
    stream: Stream, pin: impl AsyncRead + Unpin, pout: impl AsyncWrite + Unpin, is_std: bool, rh_required: bool,
    pin_required: bool,
) -> Result<()> {
    let mtu = stream.as_ref().recv_mtu().unwrap_or(8192);

    let (rh, wh) = stream.into_split();
    let mut rh = Some(rh);
    let mut wh = Some(wh);

    let mut pin = Some(pin);
    let mut pout = Some(pout);

    while rh.is_some() || pin.is_some() {
        if rh_required && rh.is_none() {
            break;
        }
        if pin_required && pin.is_none() {
            break;
        }

        let mut recv_buf = BytesMut::with_capacity(mtu as usize);
        let mut pin_buf = BytesMut::with_capacity(mtu as usize);

        select! {
            res = async {
                match rh.as_mut() {
                    Some(rh) => rh.read_buf(&mut recv_buf).await,
                    None => future::pending().await,
                }
            } => {
                match res {
                    Ok(0) | Err(_) => {
                        log::debug!("remote read failed");
                        rh = None;
                        pout = None;
                        if is_std {
                            unsafe { libc::close(STDOUT_FILENO) };
                        }
                    },
                    Ok(_) => {
                        let pout = pout.as_mut().unwrap();
                        if pout.write_all(&recv_buf).await.is_err() || pout.flush().await.is_err() {
                            log::debug!("local output failed");
                            rh = None;
                        }
                    }
                }
            },
            res = async {
                match pin.as_mut() {
                    Some(pin) => pin.read_buf(&mut pin_buf).await,
                    None => future::pending().await,
                }
            } => {
                match res {
                    Ok(0) | Err(_) => {
                        log::debug!("local input failed");
                        wh = None;
                        pin = None;
                    },
                    Ok(_) => {
                        if wh.as_mut().unwrap().write_all(&pin_buf).await.is_err() {
                            log::debug!("remote write failed");
                            pin = None;
                            if is_std {
                                unsafe { libc::close(STDIN_FILENO) };
                            }
                        }
                    }
                }
            },
        }
    }

    Ok(())
}

async fn advertise(br_edr: bool) -> Result<AdvertisementHandle> {
    let session = bluer::Session::new().await?;
    let adapter = session.default_adapter().await?;
    adapter.set_powered(true).await?;

    if br_edr {
        adapter.set_discoverable_timeout(0).await?;
        adapter.set_discoverable(true).await?;
    }

    let le_advertisement = Advertisement {
        service_uuids: vec![SERVICE_UUID].into_iter().collect(),
        discoverable: Some(true),
        ..Default::default()
    };
    Ok(adapter.advertise(le_advertisement).await?)
}

#[derive(Parser)]
struct SpeedClientOpts {
    /// Address of local Bluetooth adapter to use.
    #[clap(long, short)]
    bind: Option<Address>,
    /// Use classic Bluetooth (BR/EDR).
    /// Otherwise Bluetooth Low Energy (LE) is used.
    #[clap(long, short = 'c')]
    br_edr: bool,
    /// Measurement time in seconds.
    #[clap(long, short)]
    time: Option<u64>,
    /// Bluetooth address of target device.
    address: Address,
    /// Target PSM.
    ///
    /// For BR/EDR, it must follow the bit pattern xxxxxxx0_xxxxxxx1.
    psm: u16,
}

impl SpeedClientOpts {
    pub async fn perform(self) -> Result<()> {
        let addr_type = if self.br_edr { AddressType::BrEdr } else { AddressType::LePublic };

        let socket = Socket::new_stream()?;
        let local_sa = match self.bind {
            Some(bind_addr) => SocketAddr::new(bind_addr, addr_type, 0),
            None if self.br_edr => SocketAddr::any_br_edr(),
            None => SocketAddr::any_le(),
        };
        socket.bind(local_sa)?;

        let peer_sa = SocketAddr::new(self.address, addr_type, self.psm);
        let mut conn = socket.connect(peer_sa).await?;

        let opts = conn.as_ref().l2cap_opts();
        let conn_info = conn.as_ref().conn_info()?;
        let phy = conn.as_ref().phy()?;
        println!("Connected with {:?} and {:?} and PHYs {:#016b}", &opts, &conn_info, phy);

        let recv_mtu = conn.as_ref().recv_mtu()?;
        println!("Receive MTU is {} bytes", recv_mtu);

        let done = async {
            match self.time {
                Some(secs) => sleep(Duration::from_secs(secs)).await,
                None => future::pending().await,
            }
        };
        pin_mut!(done);

        let start = Instant::now();
        let mut total = 0;
        let mut received = VecDeque::new();
        let mut buf = vec![0; 4096];
        loop {
            tokio::select! {
                res = conn.read(&mut buf) => {
                    match res? {
                        0 => break,
                        n => {
                            total += n;
                            received.push_back((Instant::now(), n));
                        }
                    }
                }
                () = &mut done => break,
            }

            loop {
                match received.front() {
                    Some((t, _)) if t.elapsed() > Duration::from_secs(1) => {
                        received.pop_front();
                    }
                    _ => break,
                }
            }
            let avg_data: usize = received.iter().map(|(_, n)| n).sum();
            if let Some(avg_start) = received.front().map(|(t, _)| t) {
                print!("{:.1} kB/s             \r", avg_data as f32 / 1024.0 / avg_start.elapsed().as_secs_f32());
            }
        }
        let dur = start.elapsed();

        println!("                              ");
        println!(
            "Received {} kBytes in {:.1} seconds, speed is {:.1} kB/s",
            total / 1024,
            dur.as_secs_f32(),
            total as f32 / 1024.0 / dur.as_secs_f32()
        );

        Ok(())
    }
}

#[derive(Parser)]
struct SpeedServerOpts {
    /// Address of local Bluetooth adapter to use.
    #[clap(long, short)]
    bind: Option<Address>,
    /// Do not advertise device.
    #[clap(long, short)]
    no_advertise: bool,
    /// Use classic Bluetooth (BR/EDR).
    /// Otherwise Bluetooth Low Energy (LE) is used.
    #[clap(long, short = 'c')]
    br_edr: bool,
    /// Quit after one client has performed a measurement.
    #[clap(long, short)]
    once: bool,
    /// PSM to listen on.
    ///
    /// If unspecified a PSM is automatically allocated.
    psm: Option<u16>,
}

impl SpeedServerOpts {
    pub async fn perform(self) -> Result<()> {
        let _adv = if !self.no_advertise { Some(advertise(self.br_edr).await?) } else { None };

        let address_type = if self.br_edr { AddressType::BrEdr } else { AddressType::LePublic };
        let local_sa = SocketAddr::new(self.bind.unwrap_or_else(Address::any), address_type, 0);
        let listen = StreamListener::bind(local_sa).await?;

        let local_sa = listen.as_ref().local_addr()?;
        println!("Listening on PSM {}", local_sa.psm);

        loop {
            match listen.accept().await {
                Ok((mut conn, peer_sa)) => {
                    let opts = conn.as_ref().l2cap_opts();
                    let conn_info = conn.as_ref().conn_info()?;
                    let phy = conn.as_ref().phy()?;
                    println!(
                        "Connection from {} with {:?} and {:?} and PHYs {:#016b}",
                        peer_sa.addr, &opts, &conn_info, phy
                    );

                    let send_mtu = conn.as_ref().send_mtu()?;
                    println!("Send MTU is {} bytes", send_mtu);

                    loop {
                        let mut rng = rand::thread_rng();
                        let mut buf = vec![0; 4096];
                        rng.fill_bytes(&mut buf);

                        if let Err(err) = conn.write_all(&buf).await {
                            println!("Disconnected: {}", err);
                            break;
                        }
                    }
                }
                Err(err) => println!("Connection failed: {}", err),
            }

            if self.once {
                break;
            }
        }

        Ok(())
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    use tokio_compat_02::FutureExt;

    env_logger::init();
    let opts: Opts = Opts::parse();
    let result = match opts.cmd {
        Cmd::Connect(c) => c.perform().await,
        Cmd::Listen(l) => l.perform().await,
        Cmd::Serve(s) => s.perform().compat().await,
        Cmd::SpeedClient(sc) => sc.perform().await,
        Cmd::SpeedServer(ss) => ss.perform().await,
    };

    match result {
        Ok(_) => exit(0),
        Err(err) => {
            eprintln!("Error: {}", &err);
            exit(2);
        }
    }
}
