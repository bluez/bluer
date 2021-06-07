//! L2CAP sockets.

use crate::{Address, AddressType};
use dbus::arg::OwnedFd;
use libbluetooth::{bluetooth::BTPROTO_L2CAP, l2cap::sockaddr_l2};
use libc::{sockaddr, AF_BLUETOOTH, SOCK_STREAM};
use std::{
    convert::TryFrom,
    io::{Error, ErrorKind, Result},
    mem::{size_of, MaybeUninit},
    os::{
        raw::c_int,
        unix::prelude::{AsRawFd, FromRawFd, IntoRawFd, RawFd},
    },
};
use tokio::io::unix::AsyncFd;

/// First unprivileged protocol service multiplexor (PSM).
pub const PSM_DYN_START: u8 = 0x80;

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
fn bind(socket: &OwnedFd, addr: Address, addr_type: AddressType, psm: u8) -> Result<()> {
    let addr = sockaddr_l2 {
        l2_family: AF_BLUETOOTH as _,
        l2_psm: (psm as u16).to_le(),
        l2_cid: 0,
        l2_bdaddr: addr.to_bdaddr(),
        l2_bdaddr_type: addr_type.into(),
    };
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
fn accept(socket: &OwnedFd) -> Result<(OwnedFd, Address, AddressType)> {
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
    if saddr.l2_family != AF_BLUETOOTH as _ {
        return Err(Error::new(ErrorKind::InvalidInput, "address family is not Bluetooth"));
    }
    let addr = Address::from_bdaddr(saddr.l2_bdaddr);
    let addr_type = AddressType::try_from(saddr.l2_bdaddr_type)
        .map_err(|_| Error::new(ErrorKind::InvalidInput, "invalid Bluetooth address type"))?;

    Ok((fd, addr, addr_type))
}


/// An L2CAP socket server, listening for connections.
pub struct Listener {
    inner: AsyncFd<OwnedFd>,
}

impl Listener {
    /// Creates a new Listener, which will be bound to the specified local adapter
    /// address and protocol service multiplexor (PSM).
    ///
    /// Specify [Address::any] for any local adapter address.
    /// A PSM below [PSM_DYN_START] requires the `CAP_NET_BIND_SERVICE` capability.
    pub async fn bind(addr: Address, addr_type: AddressType, psm: u8) -> Result<Self> {
        let socket = socket(SOCK_STREAM)?;
        bind(&socket, addr, addr_type, psm)?;
        listen(&socket, 1024)?;
        Ok(Self { inner: AsyncFd::new(socket)? })
    }

    /// Accepts a new incoming connection from this listener.
    pub async fn accept(&self) -> Result<(Stream, Address, AddressType)> {
        let (fd, addr, addr_type) = loop {
            let mut guard = self.inner.readable().await?;
            match guard.try_io(|inner| accept(&inner.get_ref())) {
                Ok(result) => break result,
                Err(_would_block) => continue,
            }
        }?;

        let stream = Stream { inner: AsyncFd::new(fd)? };

        Ok((stream, addr, addr_type))
    }
}

/// An L2CAP stream between a local and remote socket (sequenced, reliable, two-way, connection-based).
pub struct Stream {
    inner: AsyncFd<OwnedFd>,
}

/// An L2CAP datagram socket (connectionless, unreliable messages of a fixed maximum length).
pub struct Datagram {}
