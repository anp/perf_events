extern crate errno;
extern crate failure;
#[macro_use]
extern crate failure_derive;
extern crate libc;
#[macro_use]
extern crate log;
extern crate strum;
#[macro_use]
extern crate strum_macros;

#[cfg(test)]
extern crate env_logger;

pub mod error;
pub mod events;
pub(crate) mod raw;
pub(crate) mod sys;

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io;
use std::os::unix::io::FromRawFd;

use libc::{c_int, pid_t};

pub use error::PerfEventsError;
use events::Event;

pub struct Counts {
    counters: Vec<EventCounter>,
}

impl Counts {
    pub fn new(pid: PidConfig, cpu: CpuConfig) -> CountsBuilder {
        CountsBuilder {
            pid,
            cpu,
            to_count: BTreeSet::new(),
        }
    }

    pub fn start(&mut self) -> Vec<io::Result<()>> {
        // TODO ioctl enable -- enable_on_exec?
        unimplemented!();
    }

    pub fn read(&mut self) -> Vec<(Event, u64)> {
        unimplemented!();
    }

    pub fn start_all_available() -> Result<Self, PerfEventsError> {
        let res = Counts::new(PidConfig::Current, CpuConfig::All)
            .all_available()
            .create();

        if let (_, Err(ref failures)) = res {
            for (event, error) in failures {
                debug!("error creating event {:?}: {}", event, error);
            }
        }

        if let (Ok(mut counts), _) = res {
            counts.start();
            Ok(counts)
        } else {
            // TODO return error explaining that no counters were available
            Err(PerfEventsError::StartError {
                inner: String::from("No counters started successfully."),
            })
        }
    }
}

#[derive(Debug)]
pub struct CountsBuilder {
    pid: PidConfig,
    cpu: CpuConfig,
    to_count: BTreeSet<Event>,
}

impl CountsBuilder {
    pub fn all_available(mut self) -> Self {
        for event in Event::all_events() {
            self = self.event(event);
        }

        self
    }

    pub fn event(mut self, event: Event) -> Self {
        self.to_count.insert(event);
        self
    }

    pub fn create(
        self,
    ) -> (
        Result<Counts, ()>,
        Result<(), BTreeMap<Event, sys::OpenError>>,
    ) {
        let mut counters = Vec::new();
        let mut failures = BTreeMap::new();

        for event in self.to_count {
            match EventCounter::new(event, self.pid, self.cpu) {
                Ok(c) => counters.push(c),
                Err(why) => {
                    failures.insert(event, why);
                }
            };
        }

        let ret_counts = if counters.len() == 0 {
            Err(())
        } else {
            Ok(Counts { counters })
        };

        let ret_failures = if failures.len() == 0 {
            Ok(())
        } else {
            Err(failures)
        };

        (ret_counts, ret_failures)
    }
}

#[derive(Debug)]
struct EventCounter {
    event: Event,
    file: File,
}

impl EventCounter {
    fn new(event: Event, pid: PidConfig, cpu: CpuConfig) -> Result<Self, sys::OpenError> {
        let file = unsafe { File::from_raw_fd(sys::create_fd(event, pid, cpu)?) };
        Ok(Self { event, file })
    }
}

#[derive(Clone, Copy, Debug)]
pub enum PidConfig {
    Current,
    Other(pid_t),
}

impl PidConfig {
    fn raw(&self) -> pid_t {
        match *self {
            PidConfig::Current => 0,
            PidConfig::Other(p) => p,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum CpuConfig {
    All,
    Specific(c_int),
}

impl CpuConfig {
    fn raw(&self) -> c_int {
        match *self {
            CpuConfig::All => -1,
            CpuConfig::Specific(c) => c,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use env_logger;

    #[test]
    fn test_one_shot() {
        let _ = env_logger::Builder::new()
            .filter(None, log::LevelFilter::Debug)
            .try_init();

        let mut counts = Counts::start_all_available().unwrap();
        let before = counts.read();

        println!("first:\n{:#?}", before);

        for _ in 0..10000 {
            // noop
        }

        let after = counts.read();
        println!("second:\n{:#?}", after);
    }
}
