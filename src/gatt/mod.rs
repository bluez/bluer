//! Local and remote GATT services.

use futures::ready;
use pin_project::pin_project;
use std::{
    mem::MaybeUninit,
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
///
/// When using the [AsyncRead] trait and a buffer of size less than [CharacteristicReader::mtu] bytes
/// is provided, the received characteristic value will be split over multiple
/// read operations.
/// For best efficiency provide a buffer of at least [CharacteristicReader::mtu] bytes.
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

    /// Gets the underlying UNIX socket.
    pub fn get(&self) -> &UnixStream {
        &self.stream
    }

    /// Gets the underlying UNIX socket mutably.
    pub fn get_mut(&mut self) -> &mut UnixStream {
        &mut self.stream
    }

    /// Transforms the reader into the underlying UNIX socket.
    pub fn into_inner(self) -> UnixStream {
        self.stream
    }

    /// Receive the characteristic value from a single notify or write operation.
    pub async fn recv(&self) -> std::io::Result<Vec<u8>> {
        loop {
            self.stream.readable().await?;
            let mut buf = Vec::with_capacity(self.mtu);
            match self.stream.try_read_buf(&mut buf) {
                Ok(n) => {
                    buf.truncate(n);
                    return Ok(buf);
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => continue,
                Err(err) => return Err(err),
            }
        }
    }
}

impl AsyncRead for CharacteristicReader {
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
                *this.buf = mtu_buf.split_off(buf_space);
                buf.put_slice(&mtu_buf);

                Poll::Ready(Ok(()))
            } else {
                self.project().stream.poll_read(cx, buf)
            }
        }
    }
}

/// Streams data to a characteristic with low overhead.
///
/// When using the [AsyncWrite] trait, a single write operation will send no more than
/// [CharacteristicWriter::mtu] bytes.
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

    /// Gets the underlying UNIX socket.
    pub fn get(&self) -> &UnixStream {
        &self.stream
    }

    /// Gets the underlying UNIX socket mutably.
    pub fn get_mut(&mut self) -> &mut UnixStream {
        &mut self.stream
    }

    /// Transforms the writer into the underlying UNIX socket.
    pub fn into_inner(self) -> UnixStream {
        self.stream
    }

    /// Send the characteristic value using a single write or notify operation.
    ///
    /// The length of `buf` must not exceed [CharacteristicWriter::mtu].
    pub async fn send(&self, buf: &[u8]) -> std::io::Result<()> {
        if buf.len() > self.mtu {
            return Err(std::io::Error::new(std::io::ErrorKind::WriteZero, "data length exceeds MTU"));
        }
        loop {
            self.stream.writable().await?;
            match self.stream.try_write(buf) {
                Ok(n) if n == buf.len() => return Ok(()),
                Ok(_) => return Err(std::io::Error::new(std::io::ErrorKind::Other, "partial write occured")),
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => continue,
                Err(err) => return Err(err),
            }
        }
    }
}

impl AsyncWrite for CharacteristicWriter {
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
