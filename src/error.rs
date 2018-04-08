use nix;

use sys::OpenError;

#[derive(Debug, Fail)]
pub enum PerfEventsError {
    #[fail(display = "Failed to open a perf_events file descriptor: {}", inner)]
    FdOpenError { inner: OpenError },
    #[fail(display = "Failed to start collecting metrics: {}", inner)]
    StartError { inner: String },
    #[fail(display = "Failed to enable a perf_events file descriptor: {}", inner)]
    EnableError { inner: nix::Error },
    #[fail(display = "Failed to read from a perf_events file descriptor: {}", inner)]
    ReadError { inner: ::std::io::Error },
}

impl From<::std::io::Error> for PerfEventsError {
    fn from(e: ::std::io::Error) -> PerfEventsError {
        PerfEventsError::ReadError { inner: e }
    }
}
