use std::io::prelude::*;
use std::mem::size_of;
use std::os::unix::io::AsRawFd;
use std::slice;

use super::{CpuConfig, PidConfig};
use error::*;
use events::Event;
use sys::{create_fd, perf_event_ioc_enable, PerfEventFile};

pub struct Sampler {
    event: Event,
    buffer: RingBuffer,
}

impl Sampler {
    pub fn new(event: Event, pid: PidConfig, cpu: CpuConfig) -> Result<Self> {
        let file = create_fd(event, pid, cpu)?;
        Ok(Self { event, file })
    }

    pub fn enable(&self) -> Result<()> {
        // NOTE(unsafe) this ioctl is safe if we pass a perf_event_open fd
        unsafe {
            perf_event_ioc_enable(self.file.as_raw_fd())
                .map(|_| ())
                .map_err(|e| {
                    debug!("Unable to enable a pe file descriptor: {:?}", e);
                    Error::Enable { inner: e }
                })
        }
    }
}
