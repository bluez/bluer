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
    broadcast ("broadcast"),
    read ("read"),
    write_without_response ("write-without-response"),
    write ("write"),
    notify ("notify"),
    indicate ("indicate"),
    authenticated_signed_writes ("authenticated-signed-writes"),
    extended_properties ("extended-properties"),
    reliable_write ("reliable-write"),
    writable_auxiliaries ("writable-auxiliaries"),
    encrypt_read ("encrypt-read"),
    encrypt_write ("encrypt-write"),
    encrypt_authenticated_read ("encrypt-authenticated-read"),
    encrypt_authenticated_write ("encrypt-authenticated-write"),
    secure_read ("secure-read"),
    secure_write ("secure-write"),
    authorize ("authorize"),
});

define_flags!(CharacteristicDescriptorFlags, "Bluetooth GATT characteristic descriptor flags." => {
    read ("read"),
    write ("write"),
    encrypt_read ("encrypt-read"),
    encrypt_write ("encrypt-write"),
    encrypt_authenticated_read ("encrypt-authenticated-read"),
    encrypt_authenticated_write ("encrypt-authenticated-write"),
    secure_read ("secure-read"),
    secure_write ("secure-write"),
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

/// Provides write requests to a characteristic as an IO stream.
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

/// Allows sending of notifications of a characteristic via an IO stream.
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
