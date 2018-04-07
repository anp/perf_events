use sys::OpenError;

#[derive(Debug, Fail)]
pub enum PerfEventsError {
    #[fail(display = "Failed to open a perf_events file descriptor: {}", inner)]
    FdOpenError { inner: OpenError },
    #[fail(display = "Failed to start collecting metrics: {}", inner)]
    StartError { inner: String },
}
