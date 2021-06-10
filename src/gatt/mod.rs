//! Local and remote GATT services.

use dbus::arg::OwnedFd;
use futures::ready;
use libc::{AF_LOCAL, SOCK_CLOEXEC, SOCK_NONBLOCK, SOCK_SEQPACKET};
use pin_project::pin_project;
use std::{
    mem::MaybeUninit,
    os::{
        raw::c_int,
        unix::io::{AsRawFd, FromRawFd, IntoRawFd, RawFd},
    },
    pin::Pin,
    task::{Context, Poll},
};
use strum::{Display, EnumString};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::UnixStream,
};

pub mod local;
pub mod remote;

pub(crate) const SERVICE_INTERFACE: &str = "org.bluez.GattService1";
pub(crate) const CHARACTERISTIC_INTERFACE: &str = "org.bluez.GattCharacteristic1";
pub(crate) const DESCRIPTOR_INTERFACE: &str = "org.bluez.GattDescriptor1";

define_flags!(CharacteristicFlags, "Bluetooth GATT characteristic flags." => {
    /// If set, permits broadcasts of the Characteristic Value using
    /// Server Characteristic Configuration Descriptor.
    broadcast ("broadcast"),
    /// If set allows clients to read this characteristic.
    read ("read"),
    /// If set allows clients to use the Write Request/Response operation.
    write_without_response ("write-without-response"),
    /// If set allows clients to use the Write Command ATT operation.
    write ("write"),
    /// If set allows the client to use the Handle Value Notification operation.
    notify ("notify"),
    /// If set allows the client to use the Handle Value Indication/Confirmation operation.
    indicate ("indicate"),
    /// If set allows clients to use the Signed Write Without Response procedure.
    authenticated_signed_writes ("authenticated-signed-writes"),
    /// Extended properties available.
    extended_properties ("extended-properties"),
    /// If set allows clients to use the Reliable Writes procedure.
    reliable_write ("reliable-write"),
    /// If set a client can write to the Characteristic User Description Descriptor.
    writable_auxiliaries ("writable-auxiliaries"),
    /// Require encryption for reading.
    encrypt_read ("encrypt-read"),
    /// Require encryption for writing.
    encrypt_write ("encrypt-write"),
    /// Require authentication for reading.
    encrypt_authenticated_read ("encrypt-authenticated-read"),
    /// Require authentication for writing.
    encrypt_authenticated_write ("encrypt-authenticated-write"),
    /// Require security for reading.
    secure_read ("secure-read"),
    /// Require security for writing.
    secure_write ("secure-write"),
    /// Authorize flag.
    authorize ("authorize"),
});

define_flags!(DescriptorFlags, "Bluetooth GATT characteristic descriptor flags." => {
    /// If set allows clients to read this characteristic descriptor.
    read ("read"),
    /// If set allows clients to use the Write Command ATT operation.
    write ("write"),
    /// Require encryption for reading.
    encrypt_read ("encrypt-read"),
    /// Require encryption for writing.
    encrypt_write ("encrypt-write"),
    /// Require authentication for reading.
    encrypt_authenticated_read ("encrypt-authenticated-read"),
    /// Require authentication for writing.
    encrypt_authenticated_write ("encrypt-authenticated-write"),
    /// Require security for reading.
    secure_read ("secure-read"),
    /// Require security for writing.
    secure_write ("secure-write"),
    /// Authorize flag.
    authorize ("authorize"),
});

/// Write operation type.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, EnumString, Display)]
pub enum WriteOp {
    /// Write without response.
    #[strum(serialize = "command")]
    Command,
    /// Write with response.
    #[strum(serialize = "request")]
    Request,
    /// Reliable write.
    #[strum(serialize = "reliable")]
    Reliable,
}

impl Default for WriteOp {
    fn default() -> Self {
        Self::Command
    }
}

/// Streams data from a characteristic with low overhead.
#[pin_project]
#[derive(Debug)]
pub struct CharacteristicReader {
    mtu: usize,
    #[pin]
    stream: UnixStream,
    buf: Vec<u8>,
}

impl CharacteristicReader {
    /// Maximum transmission unit.
    pub fn mtu(&self) -> usize {
        self.mtu
    }

    /// Wait for a new characteristic value to become available.
    pub async fn recvable(&self) -> std::io::Result<()> {
        self.stream.readable().await
    }

    /// Try to receive the characteristic value from a single notify or write operation.
    ///
    /// Does not wait for new data to arrive.
    pub fn try_recv(&self) -> std::io::Result<Vec<u8>> {
        let mut buf = Vec::with_capacity(self.mtu);
        let n = self.stream.try_read_buf(&mut buf)?;
        buf.truncate(n);
        Ok(buf)
    }

    /// Receive the characteristic value from a single notify or write operation.
    ///
    /// Waits for data to arrive.
    pub async fn recv(&self) -> std::io::Result<Vec<u8>> {
        loop {
            self.recvable().await?;
            match self.try_recv() {
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => continue,
                res => return res,
            }
        }
    }

    /// Consumes this object, returning the raw underlying file descriptor.
    pub fn into_raw_fd(self) -> std::io::Result<RawFd> {
        Ok(self.stream.into_std()?.into_raw_fd())
    }
}

impl AsyncRead for CharacteristicReader {
    /// Attempts to read from the characteristic value stream into `buf`.
    ///
    /// When a buffer of size less than [mtu] bytes is provided, the received
    /// characteristic value will be buffered internally and split over multiple read operations.
    /// Thus, for best efficiency, provide a buffer of at least [mtu] bytes.
    ///
    /// [mtu]: CharacteristicReader::mtu
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context, buf: &mut ReadBuf) -> Poll<std::io::Result<()>> {
        let buf_space = buf.remaining();
        if !self.buf.is_empty() {
            // Return buffered data first, if any.
            let to_read = buf_space.min(self.buf.len());
            let remaining = self.buf.split_off(to_read);
            buf.put_slice(&self.buf);
            self.buf = remaining;
            Poll::Ready(Ok(()))
        } else {
            if buf_space < self.mtu {
                let this = self.project();

                // If provided buffer is too small, read into temporary buffer.
                let mut mtu_buf: Vec<MaybeUninit<u8>> = vec![MaybeUninit::uninit(); *this.mtu];
                let mut mtu_read_buf = ReadBuf::uninit(&mut mtu_buf);
                ready!(this.stream.poll_read(cx, &mut mtu_read_buf))?;
                let n = mtu_read_buf.filled().len();
                drop(mtu_read_buf);
                mtu_buf.truncate(n);
                let mut mtu_buf: Vec<u8> = mtu_buf.into_iter().map(|v| unsafe { v.assume_init() }).collect();

                // Then fill provided buffer appropriately and keep the rest in
                // our internal buffer.
                *this.buf = mtu_buf.split_off(buf_space.min(n));
                buf.put_slice(&mtu_buf);

                Poll::Ready(Ok(()))
            } else {
                self.project().stream.poll_read(cx, buf)
            }
        }
    }
}

impl AsRawFd for CharacteristicReader {
    fn as_raw_fd(&self) -> RawFd {
        self.stream.as_raw_fd()
    }
}

impl IntoRawFd for CharacteristicReader {
    fn into_raw_fd(self) -> RawFd {
        self.into_raw_fd().expect("into_raw_fd failed")
    }
}

/// Streams data to a characteristic with low overhead.
#[pin_project]
#[derive(Debug)]
pub struct CharacteristicWriter {
    mtu: usize,
    #[pin]
    stream: UnixStream,
}

impl CharacteristicWriter {
    /// Maximum transmission unit.
    pub fn mtu(&self) -> usize {
        self.mtu
    }

    /// Waits for the remote device to stop the notification session.
    pub async fn closed(&self) -> std::io::Result<()> {
        self.stream.readable().await
    }

    /// Checks if the remote device has stopped the notification session.
    pub fn is_closed(&self) -> std::io::Result<bool> {
        let mut buf = [0u8];
        match self.stream.try_read(&mut buf) {
            Ok(_) => Ok(true),
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => Ok(false),
            Err(err) => Err(err),
        }
    }

    /// Waits for send space to become available.
    pub async fn sendable(&self) -> std::io::Result<()> {
        self.stream.writable().await
    }

    /// Tries to send the characteristic value using a single write or notify operation.
    ///
    /// The length of `buf` must not exceed [Self::mtu].
    ///
    /// Does not wait for send space to become available.
    pub fn try_send(&self, buf: &[u8]) -> std::io::Result<()> {
        if buf.len() > self.mtu {
            return Err(std::io::Error::new(std::io::ErrorKind::WriteZero, "data length exceeds MTU"));
        }
        match self.stream.try_write(buf) {
            Ok(n) if n == buf.len() => Ok(()),
            Ok(_) => Err(std::io::Error::new(std::io::ErrorKind::Other, "partial write occured")),
            Err(err) => Err(err),
        }
    }

    /// Send the characteristic value using a single write or notify operation.
    ///
    /// The length of `buf` must not exceed [Self::mtu].
    ///
    /// Waits for send space to become available.
    pub async fn send(&self, buf: &[u8]) -> std::io::Result<()> {
        loop {
            self.sendable().await?;
            match self.try_send(buf) {
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => continue,
                res => return res,
            }
        }
    }

    /// Consumes this object, returning the raw underlying file descriptor.
    pub fn into_raw_fd(self) -> std::io::Result<RawFd> {
        Ok(self.stream.into_std()?.into_raw_fd())
    }
}

impl AsyncWrite for CharacteristicWriter {
    /// Attempt to write bytes from `buf` into the characteristic value stream.
    ///
    /// A single write operation will send no more than [mtu](CharacteristicWriter::mtu) bytes.
    /// However, attempting to send a larger buffer will not result in an error but a partial send.
    fn poll_write(self: Pin<&mut Self>, cx: &mut std::task::Context, buf: &[u8]) -> Poll<std::io::Result<usize>> {
        let max_len = buf.len().min(self.mtu);
        let buf = &buf[..max_len];
        self.project().stream.poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut std::task::Context) -> Poll<std::io::Result<()>> {
        self.project().stream.poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut std::task::Context) -> Poll<std::io::Result<()>> {
        self.project().stream.poll_shutdown(cx)
    }
}

impl AsRawFd for CharacteristicWriter {
    fn as_raw_fd(&self) -> RawFd {
        self.stream.as_raw_fd()
    }
}

impl IntoRawFd for CharacteristicWriter {
    fn into_raw_fd(self) -> RawFd {
        self.into_raw_fd().expect("into_raw_fd failed")
    }
}

/// Creates a UNIX socket pair for communication with bluetoothd.
pub(crate) fn make_socket_pair() -> std::io::Result<(OwnedFd, UnixStream)> {
    let mut sv: [RawFd; 2] = [0; 2];
    if unsafe {
        libc::socketpair(AF_LOCAL, SOCK_SEQPACKET | SOCK_NONBLOCK | SOCK_CLOEXEC, 0, &mut sv as *mut c_int)
    } == -1
    {
        return Err(std::io::Error::last_os_error());
    }
    let [fd1, fd2] = sv;

    let fd1 = unsafe { OwnedFd::new(fd1) };
    let us = UnixStream::from_std(unsafe { std::os::unix::net::UnixStream::from_raw_fd(fd2) })?;

    Ok((fd1, us))
}
