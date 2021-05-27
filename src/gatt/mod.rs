//! GATT services.

use std::{
    fmt,
    pin::Pin,
    task::{Context, Poll},
};

use pin_project::pin_project;
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
pub enum WriteValueType {
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

impl Default for WriteValueType {
    fn default() -> Self {
        Self::Command
    }
}

/// Streams data from a characteristic.
#[pin_project]
pub struct CharacteristicReader {
    mtu: usize,
    #[pin]
    stream: UnixStream,
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
}

impl fmt::Debug for CharacteristicReader {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "CharacteristicReader {{ {:?} }}", &self.stream)
    }
}

impl AsyncRead for CharacteristicReader {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context, buf: &mut ReadBuf) -> Poll<std::io::Result<()>> {
        self.project().stream.poll_read(cx, buf)
    }
}

/// Streams data to a characteristic.
#[pin_project]
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

    /// Transforms the reader into the underlying UNIX socket.
    pub fn into_inner(self) -> UnixStream {
        self.stream
    }
}

impl fmt::Debug for CharacteristicWriter {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "CharacteristicWriter {{ {:?} }}", &self.stream)
    }
}

impl AsyncWrite for CharacteristicWriter {
    fn poll_write(self: Pin<&mut Self>, cx: &mut std::task::Context, buf: &[u8]) -> Poll<std::io::Result<usize>> {
        self.project().stream.poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut std::task::Context) -> Poll<std::io::Result<()>> {
        self.project().stream.poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut std::task::Context) -> Poll<std::io::Result<()>> {
        self.project().stream.poll_shutdown(cx)
    }
}
