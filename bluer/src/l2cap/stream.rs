//! Stream utility types.

use libc::{SHUT_RD, SHUT_WR};
use std::{
    fmt,
    io::Result,
    mem::ManuallyDrop,
    net::Shutdown,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use super::{shutdown, Stream};

/// Borrowed read half of [Stream], created by [Stream::split].
#[derive(Debug)]
pub struct ReadHalf<'a>(pub(crate) &'a Stream);

impl<'a> ReadHalf<'a> {
    /// Receives data on the socket from the remote address to which it is connected,
    /// without removing that data from the queue.
    /// On success, returns the number of bytes peeked.
    pub async fn peek(&self, buf: &mut [u8]) -> Result<usize> {
        self.0.socket.peek_priv(buf).await
    }

    /// Attempts to receive data on the socket, without removing that data from
    /// the queue, registering the current task for wakeup if data is not yet available.
    pub fn poll_peek(&self, cx: &mut Context, buf: &mut ReadBuf) -> Poll<Result<usize>> {
        self.0.socket.poll_peek_priv(cx, buf)
    }
}

impl<'a> AsRef<Stream> for ReadHalf<'a> {
    fn as_ref(&self) -> &Stream {
        self.0
    }
}

impl<'a> AsyncRead for ReadHalf<'a> {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context, buf: &mut ReadBuf) -> Poll<Result<()>> {
        self.0.socket.poll_recv_priv(cx, buf)
    }
}

/// Borrowed write half of [Stream], created by [Stream::split].
#[derive(Debug)]
pub struct WriteHalf<'a>(pub(crate) &'a Stream);

impl<'a> AsRef<Stream> for WriteHalf<'a> {
    fn as_ref(&self) -> &Stream {
        self.0
    }
}

impl<'a> AsyncWrite for WriteHalf<'a> {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context, buf: &[u8]) -> Poll<Result<usize>> {
        self.0.poll_write_priv(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<()>> {
        self.0.socket.poll_flush_priv(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<()>> {
        self.0.socket.poll_shutdown_priv(cx, Shutdown::Write)
    }
}

/// Error indicating that two halves were not from the same socket,
/// and thus could not be reunited.
#[derive(Debug)]
pub struct ReuniteError(pub OwnedReadHalf, pub OwnedWriteHalf);

impl fmt::Display for ReuniteError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ReuniteError")
    }
}

impl std::error::Error for ReuniteError {}

pub(crate) fn reunite(
    mut read: OwnedReadHalf, write: OwnedWriteHalf,
) -> std::result::Result<Stream, ReuniteError> {
    if Arc::ptr_eq(&read.stream, &write.stream) {
        write.forget();

        read.drop = false;
        let stream_arc = unsafe { ManuallyDrop::take(&mut read.stream) };
        Ok(Arc::try_unwrap(stream_arc).expect("Stream: try_unwrap failed"))
    } else {
        Err(ReuniteError(read, write))
    }
}

/// Owned read half of [Stream], created by [Stream::into_split].
///
/// Dropping this causes read shut down.
#[derive(Debug)]
pub struct OwnedReadHalf {
    pub(crate) stream: ManuallyDrop<Arc<Stream>>,
    pub(crate) shutdown_on_drop: bool,
    pub(crate) drop: bool,
}

impl OwnedReadHalf {
    /// Attempts to put the two halves of a stream back together.     
    pub fn reunite(self, other: OwnedWriteHalf) -> std::result::Result<Stream, ReuniteError> {
        reunite(self, other)
    }

    /// Receives data on the socket from the remote address to which it is connected,
    /// without removing that data from the queue.
    /// On success, returns the number of bytes peeked.
    pub async fn peek(&self, buf: &mut [u8]) -> Result<usize> {
        self.stream.socket.peek_priv(buf).await
    }

    /// Attempts to receive data on the socket, without removing that data from
    /// the queue, registering the current task for wakeup if data is not yet available.
    pub fn poll_peek(&self, cx: &mut Context, buf: &mut ReadBuf) -> Poll<Result<usize>> {
        self.stream.socket.poll_peek_priv(cx, buf)
    }

    /// Destroy this half, but don't close this half of the stream
    /// until the other half is dropped.
    pub fn forget(mut self) {
        self.shutdown_on_drop = false;
        drop(self);
    }
}

impl AsRef<Stream> for OwnedReadHalf {
    fn as_ref(&self) -> &Stream {
        &*self.stream
    }
}

impl AsyncRead for OwnedReadHalf {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context, buf: &mut ReadBuf) -> Poll<Result<()>> {
        self.stream.socket.poll_recv_priv(cx, buf)
    }
}

impl Drop for OwnedReadHalf {
    fn drop(&mut self) {
        if self.drop {
            if self.shutdown_on_drop {
                let _ = shutdown(self.stream.socket.fd.get_ref(), SHUT_RD);
            }
            unsafe {
                ManuallyDrop::drop(&mut self.stream);
            }
        }
    }
}

/// Owned write half of [Stream], created by [Stream::into_split].
///
/// Dropping this causes write shut down.
#[derive(Debug)]
pub struct OwnedWriteHalf {
    pub(crate) stream: Arc<Stream>,
    pub(crate) shutdown_on_drop: bool,
}

impl OwnedWriteHalf {
    /// Attempts to put the two halves of a stream back together.     
    pub fn reunite(self, other: OwnedReadHalf) -> std::result::Result<Stream, ReuniteError> {
        reunite(other, self)
    }

    /// Destroy this half, but don't close this half of the stream
    /// until the other half is dropped.
    pub fn forget(mut self) {
        self.shutdown_on_drop = false;
        drop(self);
    }
}

impl AsRef<Stream> for OwnedWriteHalf {
    fn as_ref(&self) -> &Stream {
        &*self.stream
    }
}

impl AsyncWrite for OwnedWriteHalf {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context, buf: &[u8]) -> Poll<Result<usize>> {
        self.stream.poll_write_priv(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<()>> {
        self.stream.socket.poll_flush_priv(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<()>> {
        self.stream.socket.poll_shutdown_priv(cx, Shutdown::Write)
    }
}

impl Drop for OwnedWriteHalf {
    fn drop(&mut self) {
        if self.shutdown_on_drop {
            let _ = shutdown(self.stream.socket.fd.get_ref(), SHUT_WR);
        }
    }
}
