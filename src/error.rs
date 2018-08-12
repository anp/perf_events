use failure;
use mmap;
use nix;

use fd::OpenError;

pub type Result<T> = ::std::result::Result<T, Error>;

#[derive(Debug, Fail)]
pub enum Error {
    #[fail(display = "Failed to open a perf_events file descriptor: {}", inner)]
    FdOpen { inner: OpenError },
    #[fail(display = "Failed to start collecting metrics: {}", inner)]
    Start { inner: String },
    #[fail(display = "Failed to enable a perf_events file descriptor: {}", inner)]
    Enable { inner: nix::Error },
    #[fail(display = "Failed to read from a perf_events file descriptor: {}", inner)]
    Read { inner: ::std::io::Error },
    #[fail(display = "Failed to mmap a perf_events file descriptor: {}", inner)]
    Mmap { inner: mmap::MapError },
    #[fail(display = "Encountered an unknown error: {}", inner)]
    Misc { inner: failure::Error },
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

impl From<mmap::MapError> for Error {
    fn from(inner: mmap::MapError) -> Self {
        Error::Mmap { inner }
    }
}
