use std::fs::File;
use std::io::prelude::*;
use std::mem::size_of;
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::slice;

use super::{CpuConfig, PerfEventsError, PidConfig};
use events::Event;
use sys::{create_fd, perf_event_ioc_enable, OpenError};

#[derive(Debug)]
pub struct EventCounter {
    event: Event,
    file: File,
}

impl EventCounter {
    pub fn new(event: Event, pid: PidConfig, cpu: CpuConfig) -> Result<Self, OpenError> {
        let fd = create_fd(event, pid, cpu)?;
        let file = unsafe { File::from_raw_fd(fd) };
        Ok(Self { event, file })
    }

    pub fn enable(&self) -> Result<(), PerfEventsError> {
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

        let mut value_slice = unsafe {
            let ptr = (&mut value as *mut u64) as *mut u8;
            let len = size_of::<u64>();
            slice::from_raw_parts_mut(ptr, len)
        };

        self.file.read(&mut value_slice)?;

        Ok((self.event, value))
    }
}
