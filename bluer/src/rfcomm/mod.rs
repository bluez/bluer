//! Radio frequency communication (RFCOMM) sockets and profiles.
//!
//! There are two ways to establish an RFCOMM connection:
//!
//!   1. Use a [Listener] and [Stream]. This requires knowledge of the
//!      channel number and will not register or use any SDP records.
//!   2. Register a [Profile] and listen to [connect requests](ConnectRequest) using the [ProfileHandle].
//!      This will register and discover SDP records and connect using
//!      automatically discovered channel numbers.
//!      You will probably need to register an [authorization agent](crate::agent) for this to succeed.
//!      This requires a running Bluetooth daemon.
//!

use futures::ready;
use libc::{
    c_int, AF_BLUETOOTH, EAGAIN, EINPROGRESS, MSG_PEEK, SHUT_RD, SHUT_RDWR, SHUT_WR, SOCK_RAW, SOCK_STREAM,
    SOL_BLUETOOTH, SOL_SOCKET, SO_ERROR, SO_RCVBUF, TIOCINQ, TIOCOUTQ,
};
use num_derive::{FromPrimitive, ToPrimitive};
use num_traits::FromPrimitive;
use std::{
    convert::{TryFrom, TryInto},
    fmt,
    io::{Error, ErrorKind, Result},
    mem::ManuallyDrop,
    net::Shutdown,
    os::unix::io::{AsRawFd, FromRawFd, IntoRawFd, RawFd},
    pin::Pin,
    str::FromStr,
    sync::Arc,
    task::{Context, Poll},
};
use tokio::io::{unix::AsyncFd, AsyncRead, AsyncWrite, ReadBuf};

#[cfg(feature = "bluetoothd")]
pub(crate) mod profile;

#[cfg(feature = "bluetoothd")]
pub use profile::{ConnectRequest, Profile, ProfileHandle, ReqError, ReqResult, Role};

use crate::{
    sock::{self, OwnedFd},
    sys::{
        bt_security, rfcomm_dev_req, sockaddr_rc, BTPROTO_RFCOMM, BT_SECURITY, BT_SECURITY_HIGH, BT_SECURITY_LOW,
        BT_SECURITY_MEDIUM, BT_SECURITY_SDP, RFCOMMCREATEDEV, RFCOMMRELEASEDEV, RFCOMM_CONNINFO, RFCOMM_LM,
        RFCOMM_LM_MASTER, RFCOMM_RELEASE_ONHUP, RFCOMM_REUSE_DLC, SOL_RFCOMM,
    },
    Address,
};

pub use crate::sys::rfcomm_conninfo as ConnInfo;

/// An RFCOMM socket address.
///
/// ## String representation
/// The string representation is of the form
/// `[01:23:45:67:89:0a]:75` where `01:23:45:67:89:0a` is the Bluetooth address
/// and `75` is the channel number.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SocketAddr {
    /// Device address.
    ///
    /// When listening or binding, specify [Address::any] for any local adapter address.
    pub addr: Address,
    /// Channel number.
    pub channel: u8,
}

impl SocketAddr {
    /// Creates a new RFCOMM socket address.
    pub const fn new(addr: Address, channel: u8) -> Self {
        Self { addr, channel }
    }

    /// When specified to [Socket::bind] binds to any local adapter address
    /// and a dynamically allocated channel.
    pub const fn any() -> Self {
        Self { addr: Address::any(), channel: 0 }
    }
}

impl fmt::Display for SocketAddr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "[{}]:{}", self.addr, self.channel)
    }
}

/// Invalid RFCOMM socket address error.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct InvalidSocketAddr(pub String);

impl fmt::Display for InvalidSocketAddr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "invalid RFCOMM socket address: {}", &self.0)
    }
}

impl std::error::Error for InvalidSocketAddr {}

impl FromStr for SocketAddr {
    type Err = InvalidSocketAddr;
    fn from_str(s: &str) -> std::result::Result<Self, InvalidSocketAddr> {
        let err = || InvalidSocketAddr(s.to_string());

        let (addr, channel) = s.rsplit_once(':').ok_or_else(err)?;
        let addr = addr.strip_prefix('[').and_then(|s| s.strip_suffix(']')).ok_or_else(err)?;

        Ok(Self { addr: addr.parse().map_err(|_| err())?, channel: channel.parse().map_err(|_| err())? })
    }
}

impl sock::SysSockAddr for SocketAddr {
    type SysSockAddr = sockaddr_rc;

    fn into_sys_sock_addr(self) -> Self::SysSockAddr {
        sockaddr_rc { rc_family: AF_BLUETOOTH as _, rc_bdaddr: self.addr.into(), rc_channel: self.channel }
    }

    fn try_from_sys_sock_addr(saddr: Self::SysSockAddr) -> Result<Self> {
        if saddr.rc_family != AF_BLUETOOTH as _ {
            return Err(Error::new(ErrorKind::InvalidInput, "sockaddr_rc::rc_family is not AF_BLUETOOTH"));
        }
        Ok(Self { addr: Address::from(saddr.rc_bdaddr), channel: saddr.rc_channel })
    }
}

/// RFCOMM socket security level.
#[derive(Debug, Clone, Copy, Eq, PartialEq, PartialOrd, Ord, Hash, FromPrimitive, ToPrimitive)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SecurityLevel {
    /// Insecure.
    Sdp = BT_SECURITY_SDP as _,
    /// Low.
    Low = BT_SECURITY_LOW as _,
    /// Medium.
    Medium = BT_SECURITY_MEDIUM as _,
    /// High.
    High = BT_SECURITY_HIGH as _,
}

/// RFCOMM socket security.
#[derive(Debug, Clone, Copy, Eq, PartialEq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Security {
    /// Level.
    pub level: SecurityLevel,
    /// Key size.
    pub key_size: u8,
}

impl From<Security> for bt_security {
    fn from(s: Security) -> Self {
        bt_security { level: s.level as _, key_size: s.key_size }
    }
}

impl TryFrom<bt_security> for Security {
    type Error = Error;
    fn try_from(value: bt_security) -> Result<Self> {
        Ok(Self {
            level: SecurityLevel::from_u8(value.level)
                .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "invalid bt_security::level"))?,
            key_size: value.key_size,
        })
    }
}

/// An RFCOMM socket that has not yet been converted to a [Listener] or [Stream].
///
/// The primary use of this is to configure the socket before connecting or listening.
pub struct Socket {
    fd: AsyncFd<OwnedFd>,
}

impl fmt::Debug for Socket {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Socket").field("fd", &self.fd.as_raw_fd()).finish()
    }
}

impl Socket {
    /// Creates a new socket of stream type.
    pub fn new() -> Result<Self> {
        Ok(Self { fd: AsyncFd::new(sock::socket(AF_BLUETOOTH, SOCK_STREAM, BTPROTO_RFCOMM)?)? })
    }

    /// Convert the socket into a [Listener].
    ///
    /// `backlog` defines the maximum number of pending connections are queued by the operating system
    /// at any given time.
    ///
    /// This will not register an SDP record for this channel.
    /// Register a [Bluetooth RFCOMM profile](Profile) instead, if you need a service record.
    pub fn listen(self, backlog: u32) -> Result<Listener> {
        sock::listen(
            self.fd.get_ref(),
            backlog.try_into().map_err(|_| Error::new(ErrorKind::InvalidInput, "invalid backlog"))?,
        )?;
        Ok(Listener { socket: self })
    }

    /// Establish a stream connection with a peer at the specified socket address.
    ///
    /// This requires knowledge of the channel number.
    /// Register a [Bluetooth RFCOMM profile](Profile), if you need to discover the
    /// channel number using a service record.
    pub async fn connect(self, sa: SocketAddr) -> Result<Stream> {
        self.connect_priv(sa).await?;
        Stream::from_socket(self)
    }

    /// Bind the socket to the given address.
    pub fn bind(&self, sa: SocketAddr) -> Result<()> {
        sock::bind(self.fd.get_ref(), sa)
    }

    /// Get the local address of this socket.
    pub fn local_addr(&self) -> Result<SocketAddr> {
        sock::getsockname(self.fd.get_ref())
    }

    /// Get the peer address of this socket.
    fn peer_addr_priv(&self) -> Result<SocketAddr> {
        sock::getpeername(self.fd.get_ref())
    }

    /// Get socket security.
    ///
    /// This corresponds to the `BT_SECURITY` socket option.
    pub fn security(&self) -> Result<Security> {
        let bts: bt_security = sock::getsockopt(self.fd.get_ref(), SOL_BLUETOOTH, BT_SECURITY)?;
        Security::try_from(bts)
    }

    /// Set socket security.
    ///
    /// This corresponds to the `BT_SECURITY` socket option.
    pub fn set_security(&self, security: Security) -> Result<()> {
        let bts: bt_security = security.into();
        sock::setsockopt(self.fd.get_ref(), SOL_BLUETOOTH, BT_SECURITY, &bts)
    }

    /// Gets the maximum socket receive buffer in bytes.
    ///
    /// This corresponds to the `SO_RCVBUF` socket option.
    pub fn recv_buffer(&self) -> Result<i32> {
        sock::getsockopt(self.fd.get_ref(), SOL_SOCKET, SO_RCVBUF)
    }

    /// Sets the maximum socket receive buffer in bytes.
    ///
    /// This corresponds to the `SO_RCVBUF` socket option.
    pub fn set_recv_buffer(&self, recv_buffer: i32) -> Result<()> {
        sock::setsockopt(self.fd.get_ref(), SOL_SOCKET, SO_RCVBUF, &recv_buffer)
    }

    /// Gets the RFCOMM socket connection information.
    ///
    /// This corresponds to the `RFCOMM_CONNINFO` socket option.
    pub fn conn_info(&self) -> Result<ConnInfo> {
        sock::getsockopt(self.fd.get_ref(), SOL_RFCOMM, RFCOMM_CONNINFO)
    }

    /// Gets whether the RFCOMM socket is the master.
    ///
    /// This corresponds to the `RFCOMM_LM` socket option and option bit `RFCOMM_LM_MASTER`.
    pub fn is_master(&self) -> Result<bool> {
        let opt: u32 = sock::getsockopt(self.fd.get_ref(), SOL_RFCOMM, RFCOMM_LM)?;
        Ok((opt & RFCOMM_LM_MASTER) != 0)
    }

    /// sets whether the RFCOMM socket is the master.
    ///
    /// This corresponds to the `RFCOMM_LM` socket option and option bit `RFCOMM_LM_MASTER`.
    pub fn set_master(&self, master: bool) -> Result<()> {
        let mut opt: u32 = sock::getsockopt(self.fd.get_ref(), SOL_RFCOMM, RFCOMM_LM)?;
        if master {
            opt |= RFCOMM_LM_MASTER;
        } else {
            opt &= !RFCOMM_LM_MASTER;
        }
        sock::setsockopt(self.fd.get_ref(), SOL_RFCOMM, RFCOMM_LM, &opt)
    }

    /// Get the number of bytes in the input buffer.
    ///
    /// This corresponds to the `TIOCINQ` IOCTL.
    pub fn input_buffer(&self) -> Result<u32> {
        let value: c_int = sock::ioctl_read(self.fd.get_ref(), TIOCINQ)?;
        Ok(value as _)
    }

    /// Get the number of bytes in the output buffer.
    ///
    /// This corresponds to the `TIOCOUTQ` IOCTL.
    pub fn output_buffer(&self) -> Result<u32> {
        let value: c_int = sock::ioctl_read(self.fd.get_ref(), TIOCOUTQ)?;
        Ok(value as _)
    }

    /// Creates a TTY (virtual serial port) for this RFCOMM connection.
    ///
    /// Set `dev_id` to -1 to automatically allocate an id.
    /// Returns the allocated device id.
    ///
    /// This corresponds to the `RFCOMMCREATEDEV` IOCTL.
    pub fn create_tty(&self, dev_id: i16) -> Result<i16> {
        let local_addr = self.local_addr()?;
        let remote_addr = self.peer_addr_priv()?;
        let req = rfcomm_dev_req {
            dev_id,
            flags: RFCOMM_REUSE_DLC | RFCOMM_RELEASE_ONHUP,
            src: local_addr.addr.into(),
            dst: remote_addr.addr.into(),
            channel: local_addr.channel,
        };

        let id: c_int = sock::ioctl_write(self.fd.get_ref(), RFCOMMCREATEDEV, &req)?;
        Ok(id as i16)
    }

    /// Releases a TTY (virtual serial port) for this RFCOMM connection.
    ///
    /// This corresponds to the `RFCOMMRELEASEDEV` IOCTL.
    pub fn release_tty(dev_id: i16) -> Result<()> {
        let ctl_fd = AsyncFd::new(sock::socket(AF_BLUETOOTH, SOCK_RAW, BTPROTO_RFCOMM)?)?;
        let req = rfcomm_dev_req { dev_id, flags: RFCOMM_REUSE_DLC | RFCOMM_RELEASE_ONHUP, ..Default::default() };
        sock::ioctl_write(ctl_fd.get_ref(), RFCOMMRELEASEDEV, &req)?;
        Ok(())
    }

    /// Constructs a new [Socket] from the given raw file descriptor.
    ///
    /// The file descriptor must have been set to non-blocking mode.
    ///
    /// This function *consumes ownership* of the specified file descriptor.
    /// The returned object will take responsibility for closing it when the object goes out of scope.
    ///
    /// # Safety
    /// If the passed file descriptor is invalid, undefined behavior may occur.
    pub unsafe fn from_raw_fd(fd: RawFd) -> Result<Self> {
        Ok(Self { fd: AsyncFd::new(OwnedFd::new(fd))? })
    }

    fn from_owned_fd(fd: OwnedFd) -> Result<Self> {
        Ok(Self { fd: AsyncFd::new(fd)? })
    }

    sock_priv!();
}

impl AsRawFd for Socket {
    fn as_raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }
}

impl IntoRawFd for Socket {
    fn into_raw_fd(self) -> RawFd {
        self.fd.into_inner().into_raw_fd()
    }
}

impl FromRawFd for Socket {
    /// Constructs a new instance of `Self` from the given raw file
    /// descriptor.
    ///
    /// The file descriptor must have been set to non-blocking mode.
    ///
    /// # Panics
    /// Panics when the conversion fails.
    /// Use [Socket::from_raw_fd] for a non-panicking variant.
    unsafe fn from_raw_fd(fd: RawFd) -> Self {
        Self::from_raw_fd(fd).expect("from_raw_fd failed")
    }
}

/// An RFCOMM socket server, listening for [Stream] connections.
#[derive(Debug)]
pub struct Listener {
    socket: Socket,
}

impl Listener {
    /// Creates a new Listener, which will be bound to the specified socket address.
    ///
    /// Specify [SocketAddr::any] for any local adapter address with a dynamically allocated channel.
    ///
    /// This will not register an SDP record for this channel.
    /// Register a [Bluetooth RFCOMM profile](Profile) instead, if you need a service record.
    pub async fn bind(sa: SocketAddr) -> Result<Self> {
        let socket = Socket::new()?;
        socket.bind(sa)?;
        socket.listen(1)
    }

    /// Accepts a new incoming connection from this listener.
    pub async fn accept(&self) -> Result<(Stream, SocketAddr)> {
        let (socket, sa) = self.socket.accept_priv().await?;
        Ok((Stream::from_socket(socket)?, sa))
    }

    /// Polls to accept a new incoming connection to this listener.
    pub fn poll_accept(&self, cx: &mut Context) -> Poll<Result<(Stream, SocketAddr)>> {
        let (socket, sa) = ready!(self.socket.poll_accept_priv(cx))?;
        Poll::Ready(Ok((Stream::from_socket(socket)?, sa)))
    }

    /// Constructs a new [Listener] from the given raw file descriptor.
    ///
    /// The file descriptor must have been set to non-blocking mode.
    ///
    /// This function *consumes ownership* of the specified file descriptor.
    /// The returned object will take responsibility for closing it when the object goes out of scope.
    ///
    /// # Safety
    /// If the passed file descriptor is invalid, undefined behavior may occur.
    pub unsafe fn from_raw_fd(fd: RawFd) -> Result<Self> {
        Ok(Self { socket: Socket::from_raw_fd(fd)? })
    }
}

impl AsRef<Socket> for Listener {
    fn as_ref(&self) -> &Socket {
        &self.socket
    }
}

impl AsRawFd for Listener {
    fn as_raw_fd(&self) -> RawFd {
        self.socket.as_raw_fd()
    }
}

impl FromRawFd for Listener {
    /// Constructs a new instance of `Self` from the given raw file
    /// descriptor.
    ///
    /// The file descriptor must have been set to non-blocking mode.
    ///
    /// # Panics
    /// Panics when the conversion fails.
    /// Use [Listener::from_raw_fd] for a non-panicking variant.
    unsafe fn from_raw_fd(fd: RawFd) -> Self {
        Self::from_raw_fd(fd).expect("from_raw_fd failed")
    }
}

/// An RFCOMM stream between a local and remote socket (sequenced, reliable, two-way, connection-based).
#[derive(Debug)]
pub struct Stream {
    socket: Socket,
}

impl Stream {
    /// Create Stream from Socket.
    fn from_socket(socket: Socket) -> Result<Self> {
        Ok(Self { socket })
    }

    /// Establish a stream connection with a peer at the specified socket address.
    ///
    /// Uses any local Bluetooth adapter.
    ///
    /// This requires knowledge of the channel number.
    /// Register a [Bluetooth RFCOMM profile](Profile), if you need to discover the
    /// channel number using a service record.
    pub async fn connect(addr: SocketAddr) -> Result<Self> {
        let socket = Socket::new()?;
        socket.bind(SocketAddr::any())?;
        socket.connect(addr).await
    }

    /// Gets the peer address of this stream.
    pub fn peer_addr(&self) -> Result<SocketAddr> {
        self.socket.peer_addr_priv()
    }

    /// Receives data on the socket from the remote address to which it is connected,
    /// without removing that data from the queue.
    /// On success, returns the number of bytes peeked.
    pub async fn peek(&self, buf: &mut [u8]) -> Result<usize> {
        self.socket.peek_priv(buf).await
    }

    /// Attempts to receive data on the socket, without removing that data from
    /// the queue, registering the current task for wakeup if data is not yet available.
    pub fn poll_peek(&self, cx: &mut Context, buf: &mut ReadBuf) -> Poll<Result<usize>> {
        self.socket.poll_peek_priv(cx, buf)
    }

    /// Splits the stream into a borrowed read half and a borrowed write half, which can be used
    /// to read and write the stream concurrently.
    #[allow(clippy::needless_lifetimes)]
    pub fn split<'a>(&'a mut self) -> (stream::ReadHalf<'a>, stream::WriteHalf<'a>) {
        (stream::ReadHalf(self), stream::WriteHalf(self))
    }

    /// Splits the into an owned read half and an owned write half, which can be used to read
    /// and write the stream concurrently.
    pub fn into_split(self) -> (stream::OwnedReadHalf, stream::OwnedWriteHalf) {
        let stream = Arc::new(self);
        let r = stream::OwnedReadHalf {
            stream: ManuallyDrop::new(stream.clone()),
            shutdown_on_drop: true,
            drop: true,
        };
        let w = stream::OwnedWriteHalf { stream, shutdown_on_drop: true };
        (r, w)
    }

    fn poll_write_priv(&self, cx: &mut Context, buf: &[u8]) -> Poll<Result<usize>> {
        self.socket.poll_send_priv(cx, buf)
    }

    /// Constructs a new [Stream] from the given raw file descriptor.
    ///
    /// The file descriptor must have been set to non-blocking mode.
    ///
    /// This function *consumes ownership* of the specified file descriptor.
    /// The returned object will take responsibility for closing it when the object goes out of scope.
    ///
    /// # Safety
    /// If the passed file descriptor is invalid, undefined behavior may occur.
    pub unsafe fn from_raw_fd(fd: RawFd) -> Result<Self> {
        Self::from_socket(Socket::from_raw_fd(fd)?)
    }
}

impl AsRef<Socket> for Stream {
    fn as_ref(&self) -> &Socket {
        &self.socket
    }
}

impl AsRawFd for Stream {
    fn as_raw_fd(&self) -> RawFd {
        self.socket.as_raw_fd()
    }
}

impl FromRawFd for Stream {
    /// Constructs a new instance of `Self` from the given raw file
    /// descriptor.
    ///
    /// The file descriptor must have been set to non-blocking mode.
    ///
    /// # Panics
    /// Panics when the conversion fails.
    /// Use [Stream::from_raw_fd] for a non-panicking variant.
    unsafe fn from_raw_fd(fd: RawFd) -> Self {
        Self::from_raw_fd(fd).expect("from_raw_fd failed")
    }
}

impl AsyncRead for Stream {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context, buf: &mut ReadBuf) -> Poll<Result<()>> {
        self.socket.poll_recv_priv(cx, buf)
    }
}

impl AsyncWrite for Stream {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context, buf: &[u8]) -> Poll<Result<usize>> {
        self.poll_write_priv(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<()>> {
        self.socket.poll_flush_priv(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<()>> {
        self.socket.poll_shutdown_priv(cx, Shutdown::Write)
    }
}

#[allow(clippy::duplicate_mod)]
#[path = "../stream_util.rs"]
pub mod stream;
