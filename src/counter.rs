use std::io::prelude::*;
use std::mem::size_of;
use std::os::unix::io::AsRawFd;
use std::slice;

use super::{CpuConfig, PerfEventsError, PidConfig};
use events::Event;
use sys::{create_fd, perf_event_ioc_enable, OpenError, PerfEventFile};

#[derive(Debug)]
pub struct EventCounter {
    event: Event,
    file: PerfEventFile,
}

impl EventCounter {
    pub fn new(event: Event, pid: PidConfig, cpu: CpuConfig) -> Result<Self, OpenError> {
        let file = create_fd(event, pid, cpu)?;
        Ok(Self { event, file })
    }

    pub fn enable(&self) -> Result<(), PerfEventsError> {
        // NOTE(unsafe) this ioctl is safe if we pass a perf_event_open fd
        unsafe {
            perf_event_ioc_enable(self.file.as_raw_fd())
                .map(|_| ())
                .map_err(|e| {
                    debug!("Unable to enable a pe file descriptor: {:?}", e);
                    PerfEventsError::EnableError { inner: e }
                })
        }
    }

    pub fn read(&mut self) -> Result<(Event, u64), PerfEventsError> {
        let mut value: u64 = 0;

        // NOTE(unsafe): we're just generating a pointer to a stack variable,
        // not saving that pointer beyond this stack frame
        let mut value_slice = unsafe {
            let ptr = (&mut value as *mut u64) as *mut u8;
            let len = size_of::<u64>();
            slice::from_raw_parts_mut(ptr, len)
        };

        self.file.read(&mut value_slice)?;

        Ok((self.event, value))
    }
}
