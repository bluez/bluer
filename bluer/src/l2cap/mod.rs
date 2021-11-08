//! Logical Link Control and Adaptation Protocol (L2CAP) sockets.
//!
//! L2CAP sockets provide Bluetooth Connection Oriented Channels (CoC).
//! This enables the efficient transfer of large data streams between two devices
//! using socket-oriented programming.
//!
//! L2CAP sockets work with both Bluetooth classic (BR/EDR) and Bluetooth Low Energy (LE).
//!

use crate::{
    sys::{
        bt_power, bt_security, sockaddr_l2, BTPROTO_L2CAP, BT_MODE, BT_PHY, BT_POWER, BT_POWER_FORCE_ACTIVE_OFF,
        BT_POWER_FORCE_ACTIVE_ON, BT_RCVMTU, BT_SECURITY, BT_SECURITY_FIPS, BT_SECURITY_HIGH, BT_SECURITY_LOW,
        BT_SECURITY_MEDIUM, BT_SECURITY_SDP, BT_SNDMTU, L2CAP_CONNINFO, L2CAP_LM, L2CAP_OPTIONS, SOL_L2CAP,
    },
    Address, AddressType,
};
use futures::ready;
use libc::{
    sockaddr, socklen_t, AF_BLUETOOTH, EAGAIN, EINPROGRESS, MSG_PEEK, SHUT_RD, SHUT_RDWR, SHUT_WR, SOCK_CLOEXEC,
    SOCK_DGRAM, SOCK_NONBLOCK, SOCK_SEQPACKET, SOCK_STREAM, SOL_BLUETOOTH, SOL_SOCKET, SO_ERROR, SO_RCVBUF,
    TIOCINQ, TIOCOUTQ,
};
use num_derive::{FromPrimitive, ToPrimitive};
use num_traits::FromPrimitive;
use std::{
    convert::{TryFrom, TryInto},
    fmt,
    io::{Error, ErrorKind, Result},
    marker::PhantomData,
    mem::{size_of, ManuallyDrop, MaybeUninit},
    net::Shutdown,
    os::{
        raw::{c_int, c_ulong},
        unix::io::{AsRawFd, FromRawFd, IntoRawFd, RawFd},
    },
    pin::Pin,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    task::{Context, Poll},
};
use tokio::io::{unix::AsyncFd, AsyncRead, AsyncWrite, ReadBuf};

pub use crate::sys::{l2cap_conninfo as ConnInfo, l2cap_options as Opts};

pub mod stream;

/// Possible bit values for the [link mode socket option](Socket::link_mode).
pub mod link_mode {
    pub use crate::sys::{
        L2CAP_LM_AUTH as AUTH, L2CAP_LM_ENCRYPT as ENCRYPT, L2CAP_LM_FIPS as FIPS, L2CAP_LM_MASTER as MASTER,
        L2CAP_LM_RELIABLE as RELIABLE, L2CAP_LM_SECURE as SECURE, L2CAP_LM_TRUSTED as TRUSTED,
    };
}

/// Possible bit values for the [PHY socket option](Socket::phy).
pub mod phy {
    pub use crate::sys::{
        BR1M1SLOT, BR1M3SLOT, BR1M5SLOT, EDR2M1SLOT, EDR2M3SLOT, EDR2M5SLOT, EDR3M1SLOT, EDR3M3SLOT, EDR3M5SLOT,
        LE1MRX, LE1MTX, LE2MRX, LE2MTX, LECODEDRX, LECODEDTX,
    };
}

/// File descriptor that is closed on drop.
struct OwnedFd {
    fd: RawFd,
    close_on_drop: bool,
}

impl OwnedFd {
    /// Create new OwnedFd taking ownership of file descriptor.
    pub unsafe fn new(fd: RawFd) -> Self {
        Self { fd, close_on_drop: true }
    }
}

impl AsRawFd for OwnedFd {
    fn as_raw_fd(&self) -> RawFd {
        self.fd
    }
}

impl IntoRawFd for OwnedFd {
    fn into_raw_fd(mut self) -> RawFd {
        self.close_on_drop = false;
        self.fd
    }
}

impl Drop for OwnedFd {
    fn drop(&mut self) {
        if self.close_on_drop {
            unsafe { libc::close(self.fd) };
        }
    }
}

/// First unprivileged protocol service multiplexor (PSM) for
/// Bluetooth classic (BR/EDR).
///
/// Listening on a PSM below this requires the
/// `CAP_NET_BIND_SERVICE` capability.
pub const PSM_BR_EDR_DYN_START: u16 = 0x1001;

/// First unprivileged protocol service multiplexor (PSM) for Bluetooth LE.
///
/// Listening on a PSM below this requires the
/// `CAP_NET_BIND_SERVICE` capability.
pub const PSM_LE_DYN_START: u16 = 0x80;

/// The highest protocol service multiplexor (PSM) for Bluetooth Low Energy.
pub const PSM_LE_MAX: u16 = 0xff;

/// An L2CAP socket address.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SocketAddr {
    /// Device address.
    ///
    /// Specify [Address::any] for any local adapter address.
    pub addr: Address,
    /// Device address type.
    pub addr_type: AddressType,
    /// Protocol service multiplexor (PSM).
    ///
    /// For classic Bluetooth (BR/EDR), listening on a PSM below [PSM_BR_EDR_DYN_START]
    /// requires the `CAP_NET_BIND_SERVICE` capability.
    /// The PSM must be odd and the last bit of the upper byte must be zero, i.e.
    /// it must follow the bit pattern `xxxxxxx0_xxxxxxx1` where `x` may be `1` or `0`.
    ///
    /// For Bluetooth Low Energy, listening on a PSM below [PSM_LE_DYN_START]
    /// requires the `CAP_NET_BIND_SERVICE` capability.
    /// The highest allowed PSM for LE is [PSM_LE_MAX].
    ///
    /// Set to 0 for listening to assign an available PSM.
    pub psm: u16,
    /// Connection identifier (CID).
    ///
    /// Should be set to 0.
    pub cid: u16,
}

impl SocketAddr {
    /// Creates a new L2CAP socket address.
    pub const fn new(addr: Address, addr_type: AddressType, psm: u16) -> Self {
        Self { addr, addr_type, psm, cid: 0 }
    }

    /// When specified to [Socket::bind] binds to any local adapter address
    /// using classic Bluetooth (BR/EDR) and a dynamically allocated PSM.
    pub const fn any_br_edr() -> Self {
        Self { addr: Address::any(), addr_type: AddressType::BrEdr, psm: 0, cid: 0 }
    }

    /// When specified to [Socket::bind] binds to any public, local adapter address
    /// using Bluetooth Low Energy and a dynamically allocated PSM.
    pub const fn any_le() -> Self {
        Self { addr: Address::any(), addr_type: AddressType::LePublic, psm: 0, cid: 0 }
    }
}

impl From<SocketAddr> for sockaddr_l2 {
    fn from(sa: SocketAddr) -> Self {
        sockaddr_l2 {
            l2_family: AF_BLUETOOTH as _,
            l2_psm: sa.psm.to_le(),
            l2_cid: sa.cid.to_le(),
            l2_bdaddr: sa.addr.into(),
            l2_bdaddr_type: sa.addr_type as _,
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
            addr: Address::from(saddr.l2_bdaddr),
            addr_type: AddressType::from_u8(saddr.l2_bdaddr_type)
                .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "invalid sockaddr_l2::l2_bdaddr_type"))?,
            psm: u16::from_le(saddr.l2_psm),
            cid: u16::from_le(saddr.l2_cid),
        })
    }
}

/// Creates a L2CAP socket of the specified type and returns its file descriptor.
///
/// The socket is set to non-blocking mode.
fn socket(ty: c_int) -> Result<OwnedFd> {
    let fd = match unsafe { libc::socket(AF_BLUETOOTH, ty | SOCK_NONBLOCK | SOCK_CLOEXEC, BTPROTO_L2CAP) } {
        -1 => return Err(Error::last_os_error()),
        fd => unsafe { OwnedFd::new(fd) },
    };
    Ok(fd)
}

/// Binds L2CAP socket to specified address and PSM.
fn bind(socket: &OwnedFd, sa: SocketAddr) -> Result<()> {
    let addr: sockaddr_l2 = sa.into();
    if unsafe {
        libc::bind(
            socket.as_raw_fd(),
            &addr as *const sockaddr_l2 as *const sockaddr,
            size_of::<sockaddr_l2>() as socklen_t,
        )
    } == 0
    {
        Ok(())
    } else {
        Err(Error::last_os_error())
    }
}

/// Gets the address the L2CAP socket is bound to.
fn getsockname(socket: &OwnedFd) -> Result<SocketAddr> {
    let mut saddr: MaybeUninit<sockaddr_l2> = MaybeUninit::uninit();
    let mut length = size_of::<sockaddr_l2>() as socklen_t;

    if unsafe { libc::getsockname(socket.as_raw_fd(), saddr.as_mut_ptr() as *mut _, &mut length) } == -1 {
        return Err(Error::last_os_error());
    };

    if length != size_of::<sockaddr_l2>() as socklen_t {
        return Err(Error::new(ErrorKind::InvalidInput, "invalid sockaddr_l2 length from getsockname"));
    }
    let saddr = unsafe { saddr.assume_init() };
    SocketAddr::try_from(saddr)
}

/// Gets the address the L2CAP socket is connected to.
fn getpeername(socket: &OwnedFd) -> Result<SocketAddr> {
    let mut saddr: MaybeUninit<sockaddr_l2> = MaybeUninit::uninit();
    let mut length = size_of::<sockaddr_l2>() as socklen_t;

    if unsafe { libc::getpeername(socket.as_raw_fd(), saddr.as_mut_ptr() as *mut _, &mut length) } == -1 {
        return Err(Error::last_os_error());
    };

    if length != size_of::<sockaddr_l2>() as socklen_t {
        return Err(Error::new(ErrorKind::InvalidInput, "invalid sockaddr_l2 length from getpeername"));
    }
    let saddr = unsafe { saddr.assume_init() };
    SocketAddr::try_from(saddr)
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
    let mut length = size_of::<sockaddr_l2>() as socklen_t;

    let fd = match unsafe {
        libc::accept4(socket.as_raw_fd(), saddr.as_mut_ptr() as *mut _, &mut length, SOCK_CLOEXEC | SOCK_NONBLOCK)
    } {
        -1 => return Err(Error::last_os_error()),
        fd => unsafe { OwnedFd::new(fd) },
    };

    if length != size_of::<sockaddr_l2>() as socklen_t {
        return Err(Error::new(ErrorKind::InvalidInput, "invalid sockaddr_l2 length"));
    }
    let saddr = unsafe { saddr.assume_init() };
    let sa = SocketAddr::try_from(saddr)?;

    Ok((fd, sa))
}

/// Initiate a connection on a socket to the specified address.
fn connect(socket: &OwnedFd, sa: SocketAddr) -> Result<()> {
    let addr: sockaddr_l2 = sa.into();
    if unsafe {
        libc::connect(
            socket.as_raw_fd(),
            &addr as *const sockaddr_l2 as *const sockaddr,
            size_of::<sockaddr_l2>() as socklen_t,
        )
    } == 0
    {
        Ok(())
    } else {
        Err(Error::last_os_error())
    }
}

/// Sends from buffer into socket.
fn send(socket: &OwnedFd, buf: &[u8], flags: c_int) -> Result<usize> {
    match unsafe { libc::send(socket.as_raw_fd(), buf.as_ptr() as *const _, buf.len(), flags) } {
        -1 => Err(Error::last_os_error()),
        n => Ok(n as _),
    }
}

/// Sends from buffer into socket using destination address.
fn sendto(socket: &OwnedFd, buf: &[u8], flags: c_int, sa: SocketAddr) -> Result<usize> {
    let addr: sockaddr_l2 = sa.into();
    match unsafe {
        libc::sendto(
            socket.as_raw_fd(),
            buf.as_ptr() as *const _,
            buf.len(),
            flags,
            &addr as *const sockaddr_l2 as *const sockaddr,
            size_of::<sockaddr_l2>() as socklen_t,
        )
    } {
        -1 => Err(Error::last_os_error()),
        n => Ok(n as _),
    }
}

/// Receive from socket into buffer.
fn recv(socket: &OwnedFd, buf: &mut ReadBuf, flags: c_int) -> Result<usize> {
    let unfilled = unsafe { buf.unfilled_mut() };
    match unsafe { libc::recv(socket.as_raw_fd(), unfilled.as_mut_ptr() as *mut _, unfilled.len(), flags) } {
        -1 => Err(Error::last_os_error()),
        n => {
            let n = n as usize;
            unsafe {
                buf.assume_init(n);
            }
            buf.advance(n);
            Ok(n)
        }
    }
}

/// Receive from socket into buffer with source address.
fn recvfrom(socket: &OwnedFd, buf: &mut ReadBuf, flags: c_int) -> Result<(usize, SocketAddr)> {
    let unfilled = unsafe { buf.unfilled_mut() };
    let mut saddr: MaybeUninit<sockaddr_l2> = MaybeUninit::uninit();
    let mut length = size_of::<sockaddr_l2>() as socklen_t;
    match unsafe {
        libc::recvfrom(
            socket.as_raw_fd(),
            unfilled.as_mut_ptr() as *mut _,
            unfilled.len(),
            flags,
            saddr.as_mut_ptr() as *mut _,
            &mut length,
        )
    } {
        -1 => Err(Error::last_os_error()),
        n => {
            let n = n as usize;
            unsafe {
                buf.assume_init(n);
            }
            buf.advance(n);

            if length != size_of::<sockaddr_l2>() as socklen_t {
                return Err(Error::new(ErrorKind::InvalidInput, "invalid sockaddr_l2 length"));
            }
            let saddr = unsafe { saddr.assume_init() };
            let sa = SocketAddr::try_from(saddr)?;

            Ok((n, sa))
        }
    }
}

/// Shut down part of a socket.
fn shutdown(socket: &OwnedFd, how: c_int) -> Result<()> {
    if unsafe { libc::shutdown(socket.as_raw_fd(), how) } == 0 {
        Ok(())
    } else {
        Err(Error::last_os_error())
    }
}

/// Get socket option.
fn getsockopt_level<T>(socket: &OwnedFd, level: c_int, optname: c_int) -> Result<T> {
    let mut optval: MaybeUninit<T> = MaybeUninit::uninit();
    let mut optlen: socklen_t = size_of::<T>() as _;
    if unsafe { libc::getsockopt(socket.as_raw_fd(), level, optname, optval.as_mut_ptr() as *mut _, &mut optlen) }
        == -1
    {
        return Err(Error::last_os_error());
    }
    if optlen != size_of::<T>() as _ {
        return Err(Error::new(ErrorKind::InvalidInput, "invalid size"));
    }
    let optval = unsafe { optval.assume_init() };
    Ok(optval)
}

/// Get Bluetooth level socket option.
fn getsockopt<T>(socket: &OwnedFd, optname: c_int) -> Result<T> {
    getsockopt_level(socket, SOL_BLUETOOTH, optname)
}

/// Set socket option.
fn setsockopt_level<T>(socket: &OwnedFd, level: c_int, optname: i32, optval: &T) -> Result<()> {
    let optlen: socklen_t = size_of::<T>() as _;
    if unsafe { libc::setsockopt(socket.as_raw_fd(), level, optname, optval as *const _ as *const _, optlen) }
        == -1
    {
        return Err(Error::last_os_error());
    }
    Ok(())
}

/// Set Bluetooth level socket option.
fn setsockopt<T>(socket: &OwnedFd, optname: i32, optval: &T) -> Result<()> {
    setsockopt_level(socket, SOL_BLUETOOTH, optname, optval)
}

/// Perform an IOCTL that reads a single value.
fn ioctl_read<T>(socket: &OwnedFd, request: c_ulong) -> Result<T> {
    let mut value: MaybeUninit<T> = MaybeUninit::uninit();
    if unsafe { libc::ioctl(socket.as_raw_fd(), request, value.as_mut_ptr()) } == -1 {
        return Err(Error::last_os_error());
    }
    let value = unsafe { value.assume_init() };
    Ok(value)
}

/// Any bind address for connecting to specified address.
fn any_bind_addr(addr: &SocketAddr) -> SocketAddr {
    match addr.addr_type {
        AddressType::BrEdr => SocketAddr::any_br_edr(),
        AddressType::LePublic | AddressType::LeRandom => SocketAddr::any_le(),
    }
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

/// L2CAP socket flow control mode.
#[derive(Debug, Clone, Copy, Eq, PartialEq, PartialOrd, Ord, Hash, FromPrimitive, ToPrimitive)]
pub enum FlowControl {
    /// LE flow control.
    Le = 0x03,
    /// Extended flow control.
    Extended = 0x04,
}

/// An L2CAP socket that has not yet been converted to a [StreamListener], [Stream], [SeqPacketListener],
/// [SeqPacket] or [Datagram].
///
/// The primary use of this is to configure the socket before connecting or listening.
pub struct Socket<Type> {
    fd: AsyncFd<OwnedFd>,
    _type: PhantomData<Type>,
}

impl<Type> fmt::Debug for Socket<Type> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Socket").field("fd", &self.fd.as_raw_fd()).finish()
    }
}

impl<Type> Socket<Type> {
    /// Bind the socket to the given address.
    pub fn bind(&self, sa: SocketAddr) -> Result<()> {
        bind(self.fd.get_ref(), sa)
    }

    /// Get the local address of this socket.
    pub fn local_addr(&self) -> Result<SocketAddr> {
        getsockname(self.fd.get_ref())
    }

    /// Get the peer address of this socket.
    fn peer_addr_priv(&self) -> Result<SocketAddr> {
        getpeername(self.fd.get_ref())
    }

    /// Get socket security.
    ///
    /// This corresponds to the `BT_SECURITY` socket option.
    pub fn security(&self) -> Result<Security> {
        let bts: bt_security = getsockopt(self.fd.get_ref(), BT_SECURITY)?;
        Security::try_from(bts)
    }

    /// Set socket security.
    ///
    /// This corresponds to the `BT_SECURITY` socket option.
    pub fn set_security(&self, security: Security) -> Result<()> {
        let bts: bt_security = security.into();
        setsockopt(self.fd.get_ref(), BT_SECURITY, &bts)
    }

    /// Get forced power state.
    ///
    /// This corresponds to the `BT_POWER` socket option.
    pub fn is_power_forced_active(&self) -> Result<bool> {
        let value: bt_power = getsockopt(self.fd.get_ref(), BT_POWER)?;
        Ok(value.force_active == BT_POWER_FORCE_ACTIVE_ON as _)
    }

    /// Set forced power state.
    ///
    /// This corresponds to the `BT_POWER` socket option.
    pub fn set_power_forced_active(&self, power_forced_active: bool) -> Result<()> {
        let value = bt_power {
            force_active: if power_forced_active { BT_POWER_FORCE_ACTIVE_ON } else { BT_POWER_FORCE_ACTIVE_OFF }
                as _,
        };
        setsockopt(self.fd.get_ref(), BT_POWER, &value)
    }

    /// Get maximum transmission unit (MTU) for sending.
    ///
    /// This corresponds to the `BT_SNDMTU` socket option or [Opts::omtu].
    ///
    /// Note that this value may not be available directly after an connection
    /// has been established and this function will return an error.
    /// In this case, try re-querying the MTU after send or receiving some data.
    pub fn send_mtu(&self) -> Result<u16> {
        match self.local_addr()?.addr_type {
            AddressType::BrEdr => Ok(self.l2cap_opts()?.omtu),
            _ => getsockopt(self.fd.get_ref(), BT_SNDMTU),
        }
    }

    /// Get maximum transmission unit (MTU) for receiving.
    ///
    /// This corresponds to the `BT_RCVMTU` socket option or [Opts::imtu].
    pub fn recv_mtu(&self) -> Result<u16> {
        match self.local_addr()?.addr_type {
            AddressType::BrEdr => Ok(self.l2cap_opts()?.imtu),
            _ => getsockopt(self.fd.get_ref(), BT_RCVMTU),
        }
    }

    /// Set receive MTU.
    ///
    /// This corresponds to the `BT_RCVMTU` socket option or [Opts::imtu].
    pub fn set_recv_mtu(&self, recv_mtu: u16) -> Result<()> {
        match self.local_addr()?.addr_type {
            AddressType::BrEdr => {
                let mut opts = self.l2cap_opts()?;
                opts.imtu = recv_mtu;
                self.set_l2cap_opts(&opts)
            }
            _ => setsockopt(self.fd.get_ref(), BT_RCVMTU, &recv_mtu),
        }
    }

    /// Get flow control mode.
    ///
    /// This corresponds to the `BT_MODE` socket option.
    pub fn flow_control(&self) -> Result<FlowControl> {
        let value: u8 = getsockopt(self.fd.get_ref(), BT_MODE)?;
        FlowControl::from_u8(value)
            .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "invalid flow control mode"))
    }

    /// Set flow control mode.
    ///
    /// This corresponds to the `BT_MODE` socket option.
    pub fn set_flow_control(&self, flow_control: FlowControl) -> Result<()> {
        let value = flow_control as u8;
        setsockopt(self.fd.get_ref(), BT_MODE, &value)
    }

    /// Gets the maximum socket receive buffer in bytes.
    ///
    /// This corresponds to the `SO_RCVBUF` socket option.
    pub fn recv_buffer(&self) -> Result<i32> {
        getsockopt_level(self.fd.get_ref(), SOL_SOCKET, SO_RCVBUF)
    }

    /// Sets the maximum socket receive buffer in bytes.
    ///
    /// This corresponds to the `SO_RCVBUF` socket option.
    pub fn set_recv_buffer(&self, recv_buffer: i32) -> Result<()> {
        setsockopt_level(self.fd.get_ref(), SOL_SOCKET, SO_RCVBUF, &recv_buffer)
    }

    /// Gets the raw L2CAP socket options.
    ///
    /// This corresponds to the `L2CAP_OPTIONS` socket option.
    /// This is only supported by classic sockets, i.e. [SocketAddr::addr_type] is
    /// [AddressType::BrEdr].
    pub fn l2cap_opts(&self) -> Result<Opts> {
        getsockopt_level(self.fd.get_ref(), SOL_L2CAP, L2CAP_OPTIONS)
    }

    /// Sets the raw L2CAP socket options.
    ///
    /// This corresponds to the `L2CAP_OPTIONS` socket option.
    /// This is only supported by classic sockets, i.e. [SocketAddr::addr_type] is
    /// [AddressType::BrEdr].
    pub fn set_l2cap_opts(&self, l2cap_opts: &Opts) -> Result<()> {
        setsockopt_level(self.fd.get_ref(), SOL_L2CAP, L2CAP_OPTIONS, l2cap_opts)
    }

    /// Gets the raw L2CAP link mode bit field.
    ///
    /// Possible values are defined in the [link_mode] module.
    /// This corresponds to the `L2CAP_LM` socket option.
    pub fn link_mode(&self) -> Result<i32> {
        getsockopt_level(self.fd.get_ref(), SOL_L2CAP, L2CAP_LM)
    }

    /// Sets the raw L2CAP link mode bit field.
    ///
    /// Possible values are defined in the [link_mode] module.
    /// This corresponds to the `L2CAP_LM` socket option.
    pub fn set_link_mode(&self, link_mode: i32) -> Result<()> {
        setsockopt_level(self.fd.get_ref(), SOL_L2CAP, L2CAP_LM, &link_mode)
    }

    /// Gets the L2CAP socket connection information.
    ///
    /// This corresponds to the `L2CAP_CONNINFO` socket option.
    pub fn conn_info(&self) -> Result<ConnInfo> {
        getsockopt_level(self.fd.get_ref(), SOL_L2CAP, L2CAP_CONNINFO)
    }

    /// Gets the supported PHYs bit field.
    ///
    /// Possible values are defined in the [phy] module.
    /// This corresponds to the `BT_PHY` socket option.
    pub fn phy(&self) -> Result<i32> {
        getsockopt_level(self.fd.get_ref(), SOL_BLUETOOTH, BT_PHY)
    }

    /// Get the number of bytes in the input buffer.
    ///
    /// This corresponds to the `TIOCINQ` IOCTL.
    pub fn input_buffer(&self) -> Result<u32> {
        let value: c_int = ioctl_read(self.fd.get_ref(), TIOCINQ)?;
        Ok(value as _)
    }

    /// Get the number of bytes in the output buffer.
    ///
    /// This corresponds to the `TIOCOUTQ` IOCTL.
    pub fn output_buffer(&self) -> Result<u32> {
        let value: c_int = ioctl_read(self.fd.get_ref(), TIOCOUTQ)?;
        Ok(value as _)
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
        Ok(Self { fd: AsyncFd::new(OwnedFd::new(fd))?, _type: PhantomData })
    }

    fn from_owned_fd(fd: OwnedFd) -> Result<Self> {
        Ok(Self { fd: AsyncFd::new(fd)?, _type: PhantomData })
    }

    async fn accept_priv(&self) -> Result<(Self, SocketAddr)> {
        let (fd, sa) = loop {
            let mut guard = self.fd.readable().await?;
            match guard.try_io(|inner| accept(inner.get_ref())) {
                Ok(result) => break result,
                Err(_would_block) => continue,
            }
        }?;

        let socket = Self::from_owned_fd(fd)?;
        Ok((socket, sa))
    }

    fn poll_accept_priv(&self, cx: &mut Context) -> Poll<Result<(Self, SocketAddr)>> {
        let (fd, sa) = loop {
            let mut guard = ready!(self.fd.poll_read_ready(cx))?;
            match guard.try_io(|inner| accept(inner.get_ref())) {
                Ok(result) => break result,
                Err(_would_block) => continue,
            }
        }?;

        let socket = Self::from_owned_fd(fd)?;
        Poll::Ready(Ok((socket, sa)))
    }

    async fn connect_priv(&self, sa: SocketAddr) -> Result<()> {
        match connect(self.fd.get_ref(), sa) {
            Ok(()) => Ok(()),
            Err(err) if err.raw_os_error() == Some(EINPROGRESS) || err.raw_os_error() == Some(EAGAIN) => {
                loop {
                    let mut guard = self.fd.writable().await?;
                    match guard.try_io(|inner| {
                        let err: c_int = getsockopt_level(inner.get_ref(), SOL_SOCKET, SO_ERROR)?;
                        match err {
                            0 => Ok(()),
                            EINPROGRESS | EAGAIN => Err(ErrorKind::WouldBlock.into()),
                            _ => Err(Error::from_raw_os_error(err)),
                        }
                    }) {
                        Ok(result) => break result,
                        Err(_would_block) => continue,
                    }
                }?;
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    async fn send_priv(&self, buf: &[u8]) -> Result<usize> {
        loop {
            let mut guard = self.fd.writable().await?;
            match guard.try_io(|inner| send(inner.get_ref(), buf, 0)) {
                Ok(result) => return result,
                Err(_would_block) => continue,
            }
        }
    }

    fn poll_send_priv(&self, cx: &mut Context, buf: &[u8]) -> Poll<Result<usize>> {
        loop {
            let mut guard = ready!(self.fd.poll_write_ready(cx))?;
            match guard.try_io(|inner| send(inner.get_ref(), buf, 0)) {
                Ok(result) => return Poll::Ready(result),
                Err(_would_block) => continue,
            }
        }
    }

    async fn send_to_priv(&self, buf: &[u8], target: SocketAddr) -> Result<usize> {
        loop {
            let mut guard = self.fd.writable().await?;
            match guard.try_io(|inner| sendto(inner.get_ref(), buf, 0, target)) {
                Ok(result) => return result,
                Err(_would_block) => continue,
            }
        }
    }

    fn poll_send_to_priv(&self, cx: &mut Context, buf: &[u8], target: SocketAddr) -> Poll<Result<usize>> {
        loop {
            let mut guard = ready!(self.fd.poll_write_ready(cx))?;
            match guard.try_io(|inner| sendto(inner.get_ref(), buf, 0, target)) {
                Ok(result) => return Poll::Ready(result),
                Err(_would_block) => continue,
            }
        }
    }

    async fn recv_priv(&self, buf: &mut [u8]) -> Result<usize> {
        let mut buf = ReadBuf::new(buf);
        loop {
            let mut guard = self.fd.readable().await?;
            match guard.try_io(|inner| recv(inner.get_ref(), &mut buf, 0)) {
                Ok(result) => return result,
                Err(_would_block) => continue,
            }
        }
    }

    fn poll_recv_priv(&self, cx: &mut Context, buf: &mut ReadBuf) -> Poll<Result<()>> {
        loop {
            let mut guard = ready!(self.fd.poll_read_ready(cx))?;
            match guard.try_io(|inner| recv(inner.get_ref(), buf, 0)) {
                Ok(result) => return Poll::Ready(result.map(|_| ())),
                Err(_would_block) => continue,
            }
        }
    }

    async fn recv_from_priv(&self, buf: &mut [u8]) -> Result<(usize, SocketAddr)> {
        let mut buf = ReadBuf::new(buf);
        loop {
            let mut guard = self.fd.readable().await?;
            match guard.try_io(|inner| recvfrom(inner.get_ref(), &mut buf, 0)) {
                Ok(result) => return result,
                Err(_would_block) => continue,
            }
        }
    }

    fn poll_recv_from_priv(&self, cx: &mut Context, buf: &mut ReadBuf) -> Poll<Result<SocketAddr>> {
        loop {
            let mut guard = ready!(self.fd.poll_read_ready(cx))?;
            match guard.try_io(|inner| recvfrom(inner.get_ref(), buf, 0)) {
                Ok(result) => return Poll::Ready(result.map(|(_n, sa)| sa)),
                Err(_would_block) => continue,
            }
        }
    }

    async fn peek_priv(&self, buf: &mut [u8]) -> Result<usize> {
        let mut buf = ReadBuf::new(buf);
        loop {
            let mut guard = self.fd.readable().await?;
            match guard.try_io(|inner| recv(inner.get_ref(), &mut buf, MSG_PEEK)) {
                Ok(result) => return result,
                Err(_would_block) => continue,
            }
        }
    }

    fn poll_peek_priv(&self, cx: &mut Context, buf: &mut ReadBuf) -> Poll<Result<usize>> {
        loop {
            let mut guard = ready!(self.fd.poll_read_ready(cx))?;
            match guard.try_io(|inner| recv(inner.get_ref(), buf, MSG_PEEK)) {
                Ok(result) => return Poll::Ready(result),
                Err(_would_block) => continue,
            }
        }
    }

    fn poll_flush_priv(&self, _cx: &mut Context) -> Poll<Result<()>> {
        // Flush is a no-op.
        Poll::Ready(Ok(()))
    }

    fn shutdown_priv(&self, how: Shutdown) -> Result<()> {
        let how = match how {
            Shutdown::Read => SHUT_RD,
            Shutdown::Write => SHUT_WR,
            Shutdown::Both => SHUT_RDWR,
        };
        shutdown(self.fd.get_ref(), how)?;
        Ok(())
    }

    fn poll_shutdown_priv(&self, _cx: &mut Context, how: Shutdown) -> Poll<Result<()>> {
        self.shutdown_priv(how)?;
        Poll::Ready(Ok(()))
    }
}

impl<Type> AsRawFd for Socket<Type> {
    fn as_raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }
}

impl<Type> IntoRawFd for Socket<Type> {
    fn into_raw_fd(self) -> RawFd {
        self.fd.into_inner().into_raw_fd()
    }
}

impl<Type> FromRawFd for Socket<Type> {
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

impl Socket<Stream> {
    /// Creates a new socket in of stream type.
    pub fn new_stream() -> Result<Socket<Stream>> {
        Ok(Self { fd: AsyncFd::new(socket(SOCK_STREAM)?)?, _type: PhantomData })
    }

    /// Convert the socket into a [StreamListener].
    ///
    /// `backlog` defines the maximum number of pending connections are queued by the operating system
    /// at any given time.
    pub fn listen(self, backlog: u32) -> Result<StreamListener> {
        listen(
            self.fd.get_ref(),
            backlog.try_into().map_err(|_| Error::new(ErrorKind::InvalidInput, "invalid backlog"))?,
        )?;
        Ok(StreamListener { socket: self })
    }

    /// Establish a stream connection with a peer at the specified socket address.
    pub async fn connect(self, sa: SocketAddr) -> Result<Stream> {
        self.connect_priv(sa).await?;
        Stream::from_socket(self)
    }
}

impl Socket<SeqPacket> {
    /// Creates a new socket in of sequential packet type.
    pub fn new_seq_packet() -> Result<Socket<SeqPacket>> {
        Ok(Self { fd: AsyncFd::new(socket(SOCK_SEQPACKET)?)?, _type: PhantomData })
    }

    /// Convert the socket into a [SeqPacketListener].
    ///
    /// `backlog` defines the maximum number of pending connections are queued by the operating system
    /// at any given time.
    pub fn listen(self, backlog: u32) -> Result<SeqPacketListener> {
        listen(
            self.fd.get_ref(),
            backlog.try_into().map_err(|_| Error::new(ErrorKind::InvalidInput, "invalid backlog"))?,
        )?;
        Ok(SeqPacketListener { socket: self })
    }

    /// Establish a sequential packet connection with a peer at the specified socket address.
    pub async fn connect(self, sa: SocketAddr) -> Result<SeqPacket> {
        self.connect_priv(sa).await?;
        Ok(SeqPacket { socket: self })
    }
}

impl Socket<Datagram> {
    /// Creates a new socket in of datagram type.
    pub fn new_datagram() -> Result<Socket<Datagram>> {
        Ok(Self { fd: AsyncFd::new(socket(SOCK_DGRAM)?)?, _type: PhantomData })
    }

    /// Convert the socket into a [Datagram].
    pub fn into_datagram(self) -> Datagram {
        Datagram { socket: self }
    }
}

/// An L2CAP socket server, listening for [Stream] connections.
#[derive(Debug)]
pub struct StreamListener {
    socket: Socket<Stream>,
}

impl StreamListener {
    /// Creates a new Listener, which will be bound to the specified socket address.
    ///
    /// Specify [SocketAddr::any_br_edr] or [SocketAddr::any_le] for any local adapter
    /// address with a dynamically allocated PSM.
    pub async fn bind(sa: SocketAddr) -> Result<Self> {
        let socket = Socket::<Stream>::new_stream()?;
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
}

impl AsRef<Socket<Stream>> for StreamListener {
    fn as_ref(&self) -> &Socket<Stream> {
        &self.socket
    }
}

impl AsRawFd for StreamListener {
    fn as_raw_fd(&self) -> RawFd {
        self.socket.as_raw_fd()
    }
}

/// An L2CAP stream between a local and remote socket (sequenced, reliable, two-way, connection-based).
#[derive(Debug)]
pub struct Stream {
    socket: Socket<Stream>,
    send_mtu: AtomicUsize,
}

impl Stream {
    /// Create Stream from Socket.
    fn from_socket(socket: Socket<Stream>) -> Result<Self> {
        Ok(Self { socket, send_mtu: 0.into() })
    }

    /// Establish a stream connection with a peer at the specified socket address.
    ///
    /// Uses any local Bluetooth adapter.
    pub async fn connect(addr: SocketAddr) -> Result<Self> {
        let socket = Socket::<Stream>::new_stream()?;
        socket.bind(any_bind_addr(&addr))?;
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
        // Trying to send more than the MTU on an L2CAP socket results in an error,
        // disregarding stream socket semantics. Thus we truncate the send buffer appropriately.
        // Note that no data is lost, since we return the number of actually transmitted
        // bytes and a partial write is perfectly legal.
        //
        // Additionally, the send MTU may not be available when the connection is
        // established. We handle this by assuming an MTU of 16 until it becomes
        // available.
        let send_mtu = {
            match self.send_mtu.load(Ordering::Acquire) {
                0 => match self.socket.send_mtu() {
                    Ok(mtu) => {
                        let mtu = mtu.into();
                        log::trace!("Obtained send MTU {}", mtu);
                        self.send_mtu.store(mtu, Ordering::Release);
                        mtu
                    }
                    Err(_) => {
                        log::trace!("Send MTU not yet available, assuming 16");
                        16
                    }
                },
                mtu => mtu,
            }
        };
        let max_len = buf.len().min(send_mtu);
        let buf = &buf[..max_len];

        self.socket.poll_send_priv(cx, buf)
    }
}

impl AsRef<Socket<Stream>> for Stream {
    fn as_ref(&self) -> &Socket<Stream> {
        &self.socket
    }
}

impl AsRawFd for Stream {
    fn as_raw_fd(&self) -> RawFd {
        self.socket.as_raw_fd()
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

/// An L2CAP socket server, listening for [SeqPacket] connections.
#[derive(Debug)]
pub struct SeqPacketListener {
    socket: Socket<SeqPacket>,
}

impl SeqPacketListener {
    /// Creates a new Listener, which will be bound to the specified socket address.
    ///
    /// Specify [SocketAddr::any_br_edr] or [SocketAddr::any_le] for any local adapter
    /// address with a dynamically allocated PSM.
    pub async fn bind(sa: SocketAddr) -> Result<Self> {
        let socket = Socket::<SeqPacket>::new_seq_packet()?;
        socket.bind(sa)?;
        socket.listen(1)
    }

    /// Accepts a new incoming connection from this listener.
    pub async fn accept(&self) -> Result<(SeqPacket, SocketAddr)> {
        let (socket, sa) = self.socket.accept_priv().await?;
        Ok((SeqPacket { socket }, sa))
    }

    /// Polls to accept a new incoming connection to this listener.
    pub fn poll_accept(&self, cx: &mut Context) -> Poll<Result<(SeqPacket, SocketAddr)>> {
        let (socket, sa) = ready!(self.socket.poll_accept_priv(cx))?;
        Poll::Ready(Ok((SeqPacket { socket }, sa)))
    }
}

impl AsRef<Socket<SeqPacket>> for SeqPacketListener {
    fn as_ref(&self) -> &Socket<SeqPacket> {
        &self.socket
    }
}

impl AsRawFd for SeqPacketListener {
    fn as_raw_fd(&self) -> RawFd {
        self.socket.as_raw_fd()
    }
}

/// An L2CAP sequential packet socket (sequenced, reliable, two-way connection-based data transmission path for
/// datagrams of fixed maximum length).
#[derive(Debug)]
pub struct SeqPacket {
    socket: Socket<SeqPacket>,
}

impl SeqPacket {
    /// Establish a sequential packet connection with a peer at the specified socket address.
    ///
    /// Uses any local Bluetooth adapter.
    pub async fn connect(addr: SocketAddr) -> Result<Self> {
        let socket = Socket::<SeqPacket>::new_seq_packet()?;
        socket.bind(any_bind_addr(&addr))?;
        socket.connect(addr).await
    }

    /// Gets the peer address of this stream.
    pub fn peer_addr(&self) -> Result<SocketAddr> {
        self.socket.peer_addr_priv()
    }

    /// Sends a packet.
    ///
    /// The packet length must not exceed the [Self::send_mtu].
    pub async fn send(&self, buf: &[u8]) -> Result<usize> {
        self.socket.send_priv(buf).await
    }

    /// Attempts to send a packet.
    ///
    /// The packet length must not exceed the [Self::send_mtu].
    pub fn poll_send(&self, cx: &mut Context, buf: &[u8]) -> Poll<Result<usize>> {
        self.socket.poll_send_priv(cx, buf)
    }

    /// Receives a packet.
    ///
    /// The provided buffer must be of length [Self::recv_mtu], otherwise
    /// the packet may be truncated.
    pub async fn recv(&self, buf: &mut [u8]) -> Result<usize> {
        self.socket.recv_priv(buf).await
    }

    /// Attempts to receive a packet.
    ///
    /// The provided buffer must be of length [Self::recv_mtu], otherwise
    /// the packet may be truncated.
    pub fn poll_recv(&self, cx: &mut Context, buf: &mut ReadBuf) -> Poll<Result<()>> {
        self.socket.poll_recv_priv(cx, buf)
    }

    /// Shuts down the read, write, or both halves of this connection.
    pub fn shutdown(&self, how: Shutdown) -> Result<()> {
        self.socket.shutdown_priv(how)
    }

    /// Maximum transmission unit (MTU) for sending.
    pub fn send_mtu(&self) -> Result<usize> {
        self.socket.send_mtu().map(|v| v.into())
    }

    /// Maximum transmission unit (MTU) for receiving.
    pub fn recv_mtu(&self) -> Result<usize> {
        self.socket.recv_mtu().map(|v| v.into())
    }
}

impl AsRef<Socket<SeqPacket>> for SeqPacket {
    fn as_ref(&self) -> &Socket<SeqPacket> {
        &self.socket
    }
}

impl AsRawFd for SeqPacket {
    fn as_raw_fd(&self) -> RawFd {
        self.socket.as_raw_fd()
    }
}

/// An L2CAP datagram socket (connection-less, unreliable messages of a fixed maximum length).
#[derive(Debug)]
pub struct Datagram {
    socket: Socket<Datagram>,
}

impl Datagram {
    /// Creates a new datagram socket, which will be bound to the specified socket address.
    ///
    /// Specify [SocketAddr::any_br_edr] or [SocketAddr::any_le] for any local adapter
    /// address with a dynamically allocated PSM.
    pub async fn bind(sa: SocketAddr) -> Result<Self> {
        let socket = Socket::<Datagram>::new_datagram()?;
        socket.bind(sa)?;
        Ok(socket.into_datagram())
    }

    /// Establish a datagram connection with a peer at the specified socket address.
    pub async fn connect(&self, sa: SocketAddr) -> Result<()> {
        self.socket.connect_priv(sa).await
    }

    /// Gets the peer address of this stream.
    pub fn peer_addr(&self) -> Result<SocketAddr> {
        self.socket.peer_addr_priv()
    }

    /// Sends a packet to the connected peer.
    ///
    /// The packet length must not exceed the [Self::send_mtu].
    pub async fn send(&self, buf: &[u8]) -> Result<usize> {
        self.socket.send_priv(buf).await
    }

    /// Attempts to send a packet to the connected peer.
    ///
    /// The packet length must not exceed the [Self::send_mtu].
    pub fn poll_send(&self, cx: &mut Context, buf: &[u8]) -> Poll<Result<usize>> {
        self.socket.poll_send_priv(cx, buf)
    }

    /// Sends a packet to the specified target address.
    ///
    /// The packet length must not exceed the [Self::send_mtu].
    pub async fn send_to(&self, buf: &[u8], target: SocketAddr) -> Result<usize> {
        self.socket.send_to_priv(buf, target).await
    }

    /// Attempts to send a packet to the specified target address.
    ///
    /// The packet length must not exceed the [Self::send_mtu].
    pub fn poll_send_to(&self, cx: &mut Context, buf: &[u8], target: SocketAddr) -> Poll<Result<usize>> {
        self.socket.poll_send_to_priv(cx, buf, target)
    }

    /// Receives a packet from the connected peer.
    ///
    /// The provided buffer must be of length [Self::recv_mtu], otherwise
    /// the packet may be truncated.
    pub async fn recv(&self, buf: &mut [u8]) -> Result<usize> {
        self.socket.recv_priv(buf).await
    }

    /// Attempts to receive a packet from the connected peer.
    ///
    /// The provided buffer must be of length [Self::recv_mtu], otherwise
    /// the packet may be truncated.
    pub fn poll_recv(&self, cx: &mut Context, buf: &mut ReadBuf) -> Poll<Result<()>> {
        self.socket.poll_recv_priv(cx, buf)
    }

    /// Receives a packet from anywhere.
    ///
    /// The provided buffer must be of length [Self::recv_mtu], otherwise
    /// the packet may be truncated.
    pub async fn recv_from(&self, buf: &mut [u8]) -> Result<(usize, SocketAddr)> {
        self.socket.recv_from_priv(buf).await
    }

    /// Attempts to receive a packet from anywhere.
    ///
    /// The provided buffer must be of length [Self::recv_mtu], otherwise
    /// the packet may be truncated.
    pub fn poll_recv_from(&self, cx: &mut Context, buf: &mut ReadBuf) -> Poll<Result<SocketAddr>> {
        self.socket.poll_recv_from_priv(cx, buf)
    }

    /// Shuts down the read, write, or both halves of this connection.
    pub fn shutdown(&self, how: Shutdown) -> Result<()> {
        self.socket.shutdown_priv(how)
    }

    /// Maximum transmission unit (MTU) for sending.
    pub fn send_mtu(&self) -> Result<usize> {
        self.socket.send_mtu().map(|v| v.into())
    }

    /// Maximum transmission unit (MTU) for receiving.
    pub fn recv_mtu(&self) -> Result<usize> {
        self.socket.recv_mtu().map(|v| v.into())
    }
}

impl AsRef<Socket<Datagram>> for Datagram {
    fn as_ref(&self) -> &Socket<Datagram> {
        &self.socket
    }
}

impl AsRawFd for Datagram {
    fn as_raw_fd(&self) -> RawFd {
        self.socket.as_raw_fd()
    }
}
