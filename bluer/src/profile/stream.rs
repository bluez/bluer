//! Stream utility types.

use pin_project::pin_project;
use std::{
    io::Result,
    pin::Pin,
    task::{Context, Poll},
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// Borrowed read half of [Stream](super::Stream), created by [Stream::split](super::Stream::split).
#[pin_project]
#[derive(Debug)]
pub struct ReadHalf<'a>(#[pin] pub(crate) tokio::net::unix::ReadHalf<'a>);

impl<'a> AsyncRead for ReadHalf<'a> {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context, buf: &mut ReadBuf) -> Poll<Result<()>> {
        self.project().0.poll_read(cx, buf)
    }
}

/// Borrowed write half of [Stream](super::Stream), created by [Stream::split](super::Stream::split).
#[pin_project]
#[derive(Debug)]
pub struct WriteHalf<'a>(#[pin] pub(crate) tokio::net::unix::WriteHalf<'a>);

impl<'a> AsyncWrite for WriteHalf<'a> {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context, buf: &[u8]) -> Poll<Result<usize>> {
        self.project().0.poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<()>> {
        self.project().0.poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<()>> {
        self.project().0.poll_shutdown(cx)
    }
}

/// Owned read half of [Stream](super::Stream), created by [Stream::into_split](super::Stream::into_split).
///
/// Dropping this causes read shut down.
#[pin_project]
#[derive(Debug)]
pub struct OwnedReadHalf(#[pin] pub(crate) tokio::net::unix::OwnedReadHalf);

impl AsyncRead for OwnedReadHalf {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context, buf: &mut ReadBuf) -> Poll<Result<()>> {
        self.project().0.poll_read(cx, buf)
    }
}

/// Owned write half of [Stream](super::Stream), created by [Stream::into_split](super::Stream::into_split).
///
/// Dropping this causes write shut down.
#[pin_project]
#[derive(Debug)]
pub struct OwnedWriteHalf(#[pin] pub(crate) tokio::net::unix::OwnedWriteHalf);

impl AsyncWrite for OwnedWriteHalf {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context, buf: &[u8]) -> Poll<Result<usize>> {
        self.project().0.poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<()>> {
        self.project().0.poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<()>> {
        self.project().0.poll_shutdown(cx)
    }
}
