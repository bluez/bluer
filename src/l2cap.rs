//! L2CAP sockets.

use crate::{Address, AddressType};
use dbus::arg::OwnedFd;
use futures::ready;
use libbluetooth::{
    bluetooth::{
        bt_security, BTPROTO_L2CAP, BT_DEFER_SETUP, BT_POWER, BT_POWER_FORCE_ACTIVE_OFF,
        BT_POWER_FORCE_ACTIVE_ON, BT_RCVMTU, BT_SECURITY, BT_SECURITY_FIPS, BT_SECURITY_HIGH, BT_SECURITY_LOW,
        BT_SECURITY_MEDIUM, BT_SECURITY_SDP, BT_SNDMTU,
    },
    l2cap::sockaddr_l2,
};
use libc::{sockaddr, socklen_t, AF_BLUETOOTH, SOCK_STREAM, SOL_BLUETOOTH};
use num_derive::{FromPrimitive, ToPrimitive};
use num_traits::FromPrimitive;
use std::{
    convert::{TryFrom, TryInto},
    io::{Error, ErrorKind, Result},
    mem::{size_of, MaybeUninit},
    os::{
        raw::c_int,
        unix::prelude::{AsRawFd, IntoRawFd, RawFd},
    },
    task::{Context, Poll},
};
use tokio::io::unix::AsyncFd;

/// First unprivileged protocol service multiplexor (PSM).
pub const PSM_DYN_START: u8 = 0x80;

/// An L2CAP socket address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SocketAddr {
    /// Device address.
    ///
    /// Specify [Address::any] for any local adapter address.
    pub addr: Address,
    /// Device address type.
    pub addr_type: AddressType,
    /// Protocol service multiplexor (PSM).
    ///
    /// Listening on a PSM below [PSM_DYN_START] requires the
    /// `CAP_NET_BIND_SERVICE` capability.
    pub psm: u8,
}

impl SocketAddr {
    /// Creates a new L2CAP socket address.
    pub fn new(addr: Address, addr_type: AddressType, psm: u8) -> Self {
        Self { addr, addr_type, psm }
    }
}

impl From<SocketAddr> for sockaddr_l2 {
    fn from(sa: SocketAddr) -> Self {
        sockaddr_l2 {
            l2_family: AF_BLUETOOTH as _,
            l2_psm: (sa.psm as u16).to_le(),
            l2_cid: 0,
            l2_bdaddr: sa.addr.to_bdaddr(),
            l2_bdaddr_type: sa.addr_type.into(),
        }
    }
}

impl TryFrom<sockaddr_l2> for SocketAddr {
    type Error = Error;
    fn try_from(saddr: sockaddr_l2) -> Result<Self> {
        if saddr.l2_family != AF_BLUETOOTH as _ {
            return Err(Error::new(ErrorKind::InvalidInput, "sockaddr_l2::l2_family is not AF_BLUETOOTH"));
        }
        Ok(Self {
            addr: Address::from_bdaddr(saddr.l2_bdaddr),
            addr_type: AddressType::try_from(saddr.l2_bdaddr_type)
                .map_err(|_| Error::new(ErrorKind::InvalidInput, "invalid sockaddr_l2::l2_bdaddr_type"))?,
            psm: u16::from_le(saddr.l2_psm)
                .try_into()
                .map_err(|_| Error::new(ErrorKind::InvalidInput, "invalid sockaddr_l2::l2_psm"))?,
        })
    }
}

/// Creates a L2CAP socket of the specified type and returns its file descriptors.
///
/// The socket is set to non-blocking mode.
fn socket(ty: c_int) -> Result<OwnedFd> {
    let fd = match unsafe { libc::socket(AF_BLUETOOTH, ty, BTPROTO_L2CAP) } {
        -1 => return Err(Error::last_os_error()),
        fd => unsafe { OwnedFd::new(fd) },
    };

    let mut nonblocking: c_int = 1;
    if unsafe { libc::ioctl(fd.as_raw_fd(), libc::FIONBIO, &mut nonblocking) } == -1 {
        return Err(Error::last_os_error());
    }

    Ok(fd)
}

/// Binds L2CAP socket to specified address and PSM.
fn bind(socket: &OwnedFd, sa: SocketAddr) -> Result<()> {
    let addr: sockaddr_l2 = sa.into();
    if unsafe {
        libc::bind(
            socket.as_raw_fd(),
            &addr as *const sockaddr_l2 as *const sockaddr,
            size_of::<sockaddr_l2>() as u32,
        )
    } == 0
    {
        Ok(())
    } else {
        Err(Error::last_os_error())
    }
}

/// Puts L2CAP socket in listen mode.
fn listen(socket: &OwnedFd, backlog: i32) -> Result<()> {
    if unsafe { libc::listen(socket.as_raw_fd(), backlog) } == 0 {
        Ok(())
    } else {
        Err(Error::last_os_error())
    }
}

/// Accept a connection on the provided socket.
///
/// The accepted socket is set into non-blocking mode.
fn accept(socket: &OwnedFd) -> Result<(OwnedFd, SocketAddr)> {
    let mut saddr: MaybeUninit<sockaddr_l2> = MaybeUninit::uninit();
    let mut length = size_of::<sockaddr_l2>() as libc::socklen_t;

    let fd = match unsafe {
        libc::accept4(
            socket.as_raw_fd(),
            saddr.as_mut_ptr() as *mut _,
            &mut length,
            libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
        )
    } {
        -1 => return Err(Error::last_os_error()),
        fd => unsafe { OwnedFd::new(fd) },
    };

    let saddr = unsafe { saddr.assume_init() };
    let sa = SocketAddr::try_from(saddr)?;

    Ok((fd, sa))
}

/// Get socket option.
fn getsockopt<T>(socket: &OwnedFd, optname: i32) -> Result<T> {
    let mut optval: MaybeUninit<T> = MaybeUninit::uninit();
    let mut optlen: socklen_t = size_of::<T>() as _;
    if unsafe {
        libc::getsockopt(socket.as_raw_fd(), SOL_BLUETOOTH, optname, optval.as_mut_ptr() as *mut _, &mut optlen)
    } == -1
    {
        return Err(Error::last_os_error());
    }
    if optlen != size_of::<T>() as _ {
        return Err(Error::new(ErrorKind::InvalidInput, "invalid size"));
    }
    let optval = unsafe { optval.assume_init() };
    Ok(optval)
}

/// Set socket option.
fn setsockopt<T>(socket: &OwnedFd, optname: i32, optval: &T) -> Result<()> {
    let optlen: socklen_t = size_of::<T>() as _;
    if unsafe {
        libc::setsockopt(socket.as_raw_fd(), SOL_BLUETOOTH, optname, optval as *const _ as *const _, optlen)
    } == -1
    {
        return Err(Error::last_os_error());
    }
    Ok(())
}

/// L2CAP socket security level.
#[derive(Debug, Clone, Copy, Eq, PartialEq, PartialOrd, Ord, Hash, FromPrimitive, ToPrimitive)]
pub enum SecurityLevel {
    /// Insecure.
    Sdp = BT_SECURITY_SDP as _,
    /// Low: authentication is requested.
    Low = BT_SECURITY_LOW as _,
    /// Medium.
    Medium = BT_SECURITY_MEDIUM as _,
    /// High.
    High = BT_SECURITY_HIGH as _,
    /// FIPS.
    Fips = BT_SECURITY_FIPS as _,
}

/// L2CAP socket security.
#[derive(Debug, Clone, Copy, Eq, PartialEq, PartialOrd, Ord, Hash)]
pub struct Security {
    /// Level.
    level: SecurityLevel,
    /// Key size.
    key_size: u8,
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
                .ok_or(Error::new(ErrorKind::InvalidInput, "invalid bt_security::level"))?,
            key_size: value.key_size,
        })
    }
}

/// L2CAP socket flow control mode.
#[derive(Debug, Clone, Copy, Eq, PartialEq, PartialOrd, Ord, Hash, FromPrimitive, ToPrimitive)]
pub enum FlowControl {
    /// LE flow control.
    Le = 0x03,
    /// Extended flow control.
    Extended = 0x04,
}

#[repr(C)]
struct bt_power {
    force_active: u8,
}

const BT_MODE: i32 = 15;

/// An L2CAP socket that has not yet been converted to a [StreamListener], [Stream], [SeqPacketListener],
/// [SeqPacket] or [Datagram].
pub struct Socket {
    inner: AsyncFd<OwnedFd>,
}

impl Socket {
    /// Get socket security.
    pub fn security(&self) -> Result<Security> {
        let bts: bt_security = getsockopt(self.inner.get_ref(), BT_SECURITY)?;
        Security::try_from(bts)
    }

    /// Set socket security.
    pub fn set_security(&self, security: Security) -> Result<()> {
        let bts: bt_security = security.into();
        setsockopt(self.inner.get_ref(), BT_SECURITY, &bts)
    }

    /// Get defer setup state.
    pub fn is_defer_setup(&self) -> Result<bool> {
        let value: u32 = getsockopt(self.inner.get_ref(), BT_DEFER_SETUP)?;
        Ok(value != 0)
    }

    /// Set defer setup state.
    pub fn set_defer_setup(&self, defer_setup: bool) -> Result<()> {
        let value: u32 = defer_setup.into();
        setsockopt(self.inner.get_ref(), BT_DEFER_SETUP, &value)
    }

    /// Get forced power state.
    pub fn is_power_forced_active(&self) -> Result<bool> {
        let value: bt_power = getsockopt(self.inner.get_ref(), BT_POWER)?;
        Ok(value.force_active == BT_POWER_FORCE_ACTIVE_ON as _)
    }

    /// Set forced power state.
    pub fn set_power_forced_active(&self, power_forced_active: bool) -> Result<()> {
        let value = bt_power {
            force_active: if power_forced_active { BT_POWER_FORCE_ACTIVE_ON } else { BT_POWER_FORCE_ACTIVE_OFF }
                as _,
        };
        setsockopt(self.inner.get_ref(), BT_POWER, &value)
    }

    /// Send MTU.
    pub fn send_mtu(&self) -> Result<u16> {
        getsockopt(self.inner.get_ref(), BT_SNDMTU)
    }

    /// Receive MTU.
    pub fn recv_mtu(&self) -> Result<u16> {
        getsockopt(self.inner.get_ref(), BT_RCVMTU)
    }

    /// Set receive MTU.
    pub fn set_recv_mtu(&self, recv_mtu: u16) -> Result<()> {
        setsockopt(self.inner.get_ref(), BT_RCVMTU, &recv_mtu)
    }

    /// Get flow control mode.
    pub fn flow_control(&self) -> Result<FlowControl> {
        let value: u8 = getsockopt(self.inner.get_ref(), BT_MODE)?;
        FlowControl::from_u8(value).ok_or(Error::new(ErrorKind::InvalidInput, "invalid flow control mode"))
    }

    /// Set flow control mode.
    pub fn set_flow_control(&self, flow_control: FlowControl) -> Result<()> {
        let value = flow_control as u8;
        setsockopt(self.inner.get_ref(), BT_MODE, &value)
    }
}

/// An L2CAP socket server, listening for [Stream] connections.
pub struct StreamListener {
    inner: AsyncFd<OwnedFd>,
}

impl StreamListener {
    /// Creates a new Listener, which will be bound to the specified socket address.
    ///
    /// Specify [Address::any] for any local adapter address.
    /// A PSM below [PSM_DYN_START] requires the `CAP_NET_BIND_SERVICE` capability.
    pub async fn bind(sa: SocketAddr) -> Result<Self> {
        let socket = socket(SOCK_STREAM)?;
        bind(&socket, sa)?;
        listen(&socket, 1)?;
        Ok(Self { inner: AsyncFd::new(socket)? })
    }

    /// Accepts a new incoming connection from this listener.
    pub async fn accept(&self) -> Result<(Stream, SocketAddr)> {
        let (fd, sa) = loop {
            let mut guard = self.inner.readable().await?;
            match guard.try_io(|inner| accept(&inner.get_ref())) {
                Ok(result) => break result,
                Err(_would_block) => continue,
            }
        }?;

        let stream = Stream { inner: AsyncFd::new(fd)? };

        Ok((stream, sa))
    }

    /// Polls to accept a new incoming connection to this listener.
    pub fn poll_accept(&self, cx: &mut Context) -> Poll<Result<(Stream, SocketAddr)>> {
        let (fd, sa) = loop {
            let mut guard = ready!(self.inner.poll_read_ready(cx))?;
            match guard.try_io(|inner| accept(&inner.get_ref())) {
                Ok(result) => break result,
                Err(_would_block) => continue,
            }
        }?;

        let stream = Stream { inner: AsyncFd::new(fd)? };

        Poll::Ready(Ok((stream, sa)))
    }

    /// Constructs a new Listener from the given raw file descriptor.
    ///
    /// This function *consumes ownership* of the specified file descriptor.
    /// The returned object will take responsibility for closing it when the object goes out of scope.
    pub unsafe fn from_raw_fd(fd: RawFd) -> Result<Self> {
        Ok(Self { inner: AsyncFd::new(OwnedFd::new(fd))? })
    }
}

impl AsRawFd for StreamListener {
    fn as_raw_fd(&self) -> RawFd {
        self.inner.as_raw_fd()
    }
}

impl IntoRawFd for StreamListener {
    fn into_raw_fd(self) -> RawFd {
        self.inner.into_inner().into_raw_fd()
    }
}

/// An L2CAP stream between a local and remote socket (sequenced, reliable, two-way, connection-based).
pub struct Stream {
    inner: AsyncFd<OwnedFd>,
}

impl Stream {
    // pub async fn connect(addr: SocketAddr) -> Result<Self> {
    //
    // }
}

/// An L2CAP socket server, listening for [SeqPacket] connections.
pub struct SeqPacketListener {}

/// An L2CAP sequential packet socket (sequenced, reliable, two-way connection-based data transmission path for
/// datagrams of fixed maximum length).
pub struct SeqPacket {}

/// An L2CAP datagram socket (connectionless, unreliable messages of a fixed maximum length).
pub struct Datagram {}
