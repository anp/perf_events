use failure;
use nix;

use fd::{FileControlError, OpenError};
use sample::ring_buffer::BufferError;

pub type Result<T> = ::std::result::Result<T, Error>;

#[derive(Debug, Fail)]
pub enum Error {
    #[fail(display = "Failed to open a perf_events file descriptor: {}", inner)]
    FdOpen { inner: OpenError },
    #[fail(display = "Failed to start collecting metrics: {}", inner)]
    Start { inner: String },
    #[fail(display = "Failed to interact with a POSIX API: {}", inner)]
    Posix { inner: nix::Error },
    #[fail(display = "Failed to read from a perf_events file descriptor: {}", inner)]
    Read { inner: ::std::io::Error },
    #[fail(display = "Failed to mmap a perf_events file descriptor: {}", inner)]
    Mmap { inner: BufferError },
    #[fail(display = "Failed to call fcntl on a perf_events file descriptor: {}", inner)]
    Fcntl { inner: FileControlError },
    #[fail(display = "Encountered an unknown error: {}", inner)]
    Misc { inner: failure::Error },
}

impl From<nix::Error> for Error {
    fn from(inner: nix::Error) -> Self {
        Error::Posix { inner }
    }
}

impl From<BufferError> for Error {
    fn from(inner: BufferError) -> Self {
        Error::Mmap { inner }
    }
}

impl From<FileControlError> for Error {
    fn from(inner: FileControlError) -> Self {
        Error::Fcntl { inner }
    }
}

impl From<failure::Error> for Error {
    fn from(inner: failure::Error) -> Self {
        Error::Misc { inner }
    }
}

impl From<OpenError> for Error {
    fn from(inner: OpenError) -> Self {
        Error::FdOpen { inner }
    }
}

impl From<::std::io::Error> for Error {
    fn from(inner: ::std::io::Error) -> Self {
        Error::Read { inner }
    }
}
