//! Arbitrary RFCOMM connections and listens.

use bluer::{
    agent::Agent,
    id::ServiceClass,
    rfcomm::{Listener, Profile, ReqError, Role, Socket, SocketAddr, Stream},
    AdapterEvent, Address, Uuid,
};
use bytes::BytesMut;
use clap::Parser;
use crossterm::{terminal, tty::IsTty};
use futures::{future, pin_mut, StreamExt};
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
    io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader},
    select,
    time::sleep,
};
use tokio_compat_02::IoCompat;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Parser)]
#[clap(
    name = "rfcat",
    about = "Arbitrary Bluetooth RFCOMM connections and listens.",
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
    /// Allocate a TTY for the RFCOMM connection.
    #[clap(long)]
    tty: bool,
    /// Public Bluetooth address of target device.
    address: Address,
    /// Target RFCOMM channel.
    #[clap(long, short)]
    channel: Option<u8>,
    /// Target RFCOMM profile.
    #[clap(long, short)]
    profile: Option<Uuid>,
}

impl ConnectOpts {
    pub async fn perform(self) -> Result<()> {
        let stream = match (self.channel, self.profile) {
            (Some(channel), None) => {
                let socket = Socket::new()?;
                let local_sa = match self.bind {
                    Some(bind_addr) => SocketAddr::new(bind_addr, 0),
                    None => SocketAddr::any(),
                };
                socket.bind(local_sa)?;

                let peer_sa = SocketAddr::new(self.address, channel);
                socket.connect(peer_sa).await?
            }
            (None, uuid_opt) => {
                let uuid = uuid_opt.unwrap_or(ServiceClass::SerialPort.into());

                let session = bluer::Session::new().await?;
                let adapter_names = session.adapter_names().await?;
                let adapter_name = adapter_names.first().ok_or("no Bluetooth adapter present")?;
                let adapter = session.adapter(adapter_name)?;
                adapter.set_powered(true).await?;
                adapter.set_pairable(false).await?;

                let agent = Agent::default();
                let _agent_hndl = session.register_agent(agent).await?;

                let profile = Profile {
                    uuid,
                    name: Some("rfcat client".to_string()),
                    role: Some(Role::Client),
                    require_authentication: Some(false),
                    require_authorization: Some(false),
                    auto_connect: Some(true),
                    ..Default::default()
                };
                let mut hndl = session.register_profile(profile).await?;

                eprintln!("Discovering device...");
                let mut devs = adapter.discover_devices().await?;
                while let Some(evt) = devs.next().await {
                    if let AdapterEvent::DeviceAdded(addr) = evt {
                        if addr == self.address {
                            break;
                        }
                    }
                }
                let dev = adapter.device(self.address)?;
                drop(devs);

                eprintln!("Connecting profile...");
                loop {
                    tokio::select! {
                        res = async {
                            let _ = dev.connect().await;
                            dev.connect_profile(&uuid).await
                        } => {
                            if let Err(err) = res {
                                eprintln!("Connect profile failed: {}", err);
                            }
                            sleep(Duration::from_secs(3)).await;
                        },
                        req = hndl.next() => {
                            let req = req.unwrap();
                            eprintln!("Connect request from {}", req.device());
                            if req.device() == self.address {
                                eprintln!("Accepting request...");
                                break req.accept()?;
                            } else {
                                req.reject(ReqError::Rejected);
                            }
                        },
                    }
                }
            }
            _ => {
                eprintln!("either channel or profile must be specified");
                exit(1);
            }
        };

        if self.tty {
            let tty = stream.as_ref().create_tty(-1)?;
            println!("Allocated TTY {}", tty);

            println!("Press enter to release TTY and exit");
            let stdin = BufReader::new(tokio::io::stdin());
            let mut lines = stdin.lines();
            let _ = lines.next_line().await;

            Socket::release_tty(tty as _)?;
        } else {
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
    /// Channel to listen on.
    /// Specify 0 to auto allocate an available channel.
    #[clap(long, short)]
    channel: Option<u8>,
    /// UUID of RFCOMM profile to create.
    #[clap(long, short)]
    profile: Option<Uuid>,
}

impl ListenOpts {
    pub async fn perform(self) -> Result<()> {
        let stream = match (self.channel, self.profile) {
            (Some(channel), None) => {
                let local_sa = SocketAddr::new(self.bind.unwrap_or_else(Address::any), channel);
                let listen = Listener::bind(local_sa).await?;
                let local_sa = listen.as_ref().local_addr()?;
                if self.verbose && channel == 0 {
                    eprintln!("Listening on channel {}", local_sa.channel);
                }
                let (stream, peer_sa) = listen.accept().await?;
                if self.verbose {
                    eprintln!("Connected from {}", peer_sa.addr);
                }
                stream
            }
            (None, uuid_opt) => {
                let uuid = uuid_opt.unwrap_or(ServiceClass::SerialPort.into());

                let session = bluer::Session::new().await?;
                let adapter_names = session.adapter_names().await?;
                let adapter_name = adapter_names.first().ok_or("no Bluetooth adapter present")?;
                let adapter = session.adapter(adapter_name)?;
                adapter.set_powered(true).await?;
                adapter.set_discoverable(true).await?;
                adapter.set_discoverable_timeout(0).await?;
                adapter.set_pairable(false).await?;

                let agent = Agent::default();
                let _agent_hndl = session.register_agent(agent).await?;

                let profile = Profile {
                    uuid,
                    name: Some("rfcat listener".to_string()),
                    channel: Some(0),
                    role: Some(Role::Server),
                    require_authentication: Some(false),
                    require_authorization: Some(false),
                    ..Default::default()
                };
                let mut hndl = session.register_profile(profile).await?;

                eprintln!("Registered profile");

                let req = hndl.next().await.expect("received no connect request");
                eprintln!("Connect from {}", req.device());
                req.accept()?
            }
            _ => {
                eprintln!("either channel or profile must be specified");
                exit(1);
            }
        };

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
    /// Exit after handling one connection.
    #[clap(long, short)]
    one_shot: bool,
    /// Allocate a pseudo-terminal (PTY) for the program.
    /// Use together with --raw when connecting.
    #[clap(long)]
    pty: bool,
    /// Channel to listen on.
    /// Specify 0 to auto allocate an available channel.
    #[clap(long, short)]
    channel: Option<u8>,
    /// UUID of RFCOMM profile to create.
    #[clap(long, short)]
    profile: Option<Uuid>,
    /// Program to execute once connection is established.
    command: OsString,
    /// Arguments to program.
    args: Vec<OsString>,
}

impl ServeOpts {
    pub async fn perform(self) -> Result<()> {
        use tab_pty_process::CommandExt;

        let mut listener = None;
        let mut hndl = None;
        let _agent_hndl;

        match (self.channel, self.profile) {
            (Some(channel), None) => {
                let local_sa = SocketAddr::new(self.bind.unwrap_or_else(Address::any), channel);
                let listen = Listener::bind(local_sa).await?;
                let local_sa = listen.as_ref().local_addr()?;
                if self.verbose && channel == 0 {
                    eprintln!("Listening on channel {}", local_sa.channel);
                }
                listener = Some(listen);
            }
            (None, uuid_opt) => {
                let uuid = uuid_opt.unwrap_or(ServiceClass::SerialPort.into());

                let session = bluer::Session::new().await?;
                let adapter_names = session.adapter_names().await?;
                let adapter_name = adapter_names.first().ok_or("no Bluetooth adapter present")?;
                let adapter = session.adapter(adapter_name)?;
                adapter.set_powered(true).await?;
                adapter.set_discoverable(true).await?;
                adapter.set_discoverable_timeout(0).await?;
                adapter.set_pairable(true).await?;

                let agent = Agent::default();
                _agent_hndl = session.register_agent(agent).await?;

                let profile = Profile {
                    uuid,
                    name: Some("rfcat server".to_string()),
                    channel: Some(0),
                    role: Some(Role::Server),
                    require_authentication: Some(false),
                    require_authorization: Some(false),
                    auto_connect: Some(true),
                    ..Default::default()
                };
                hndl = Some(session.register_profile(profile).await?);
                eprintln!("Registered profile");
            }
            _ => {
                eprintln!("either channel or profile must be specified");
                exit(1);
            }
        };

        loop {
            let stream = match (&mut listener, &mut hndl) {
                (Some(listener), None) => {
                    let (stream, peer_sa) = listener.accept().await?;
                    if self.verbose {
                        eprintln!("Connected from {}", peer_sa.addr);
                    }
                    stream
                }
                (None, Some(hndl)) => {
                    let req = hndl.next().await.expect("received no connect request");
                    eprintln!("Connect from {}", req.device());
                    req.accept()?
                }
                _ => unreachable!(),
            };

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
    let mtu = 8192;

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

#[derive(Parser)]
struct SpeedClientOpts {
    /// Address of local Bluetooth adapter to use.
    #[clap(long, short)]
    bind: Option<Address>,
    /// Measurement time in seconds.
    #[clap(long, short)]
    time: Option<u64>,
    /// Bluetooth address of target device.
    address: Address,
    /// Target channel.
    channel: u8,
}

impl SpeedClientOpts {
    pub async fn perform(self) -> Result<()> {
        let socket = Socket::new()?;
        let local_sa = match self.bind {
            Some(bind_addr) => SocketAddr::new(bind_addr, 0),
            None => SocketAddr::any(),
        };
        socket.bind(local_sa)?;

        let peer_sa = SocketAddr::new(self.address, self.channel);
        let mut conn = socket.connect(peer_sa).await?;

        let conn_info = conn.as_ref().conn_info()?;
        println!("Connected with {:?}", &conn_info);

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
    /// Quit after one client has performed a measurement.
    #[clap(long, short)]
    once: bool,
    /// Channel to listen on.
    channel: u8,
}

impl SpeedServerOpts {
    pub async fn perform(self) -> Result<()> {
        let session = bluer::Session::new().await?;
        let adapter_names = session.adapter_names().await?;
        let adapter_name = adapter_names.first().ok_or("no Bluetooth adapter present")?;
        let adapter = session.adapter(adapter_name)?;
        adapter.set_powered(true).await?;
        adapter.set_discoverable(true).await?;
        adapter.set_discoverable_timeout(0).await?;

        let local_sa = SocketAddr::new(self.bind.unwrap_or_else(Address::any), self.channel);
        let listen = Listener::bind(local_sa).await?;

        let local_sa = listen.as_ref().local_addr()?;
        println!("Listening on channel {}", local_sa.channel);

        loop {
            match listen.accept().await {
                Ok((mut conn, peer_sa)) => {
                    let conn_info = conn.as_ref().conn_info()?;
                    println!("Connection from {} with {:?}", peer_sa.addr, &conn_info,);

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
