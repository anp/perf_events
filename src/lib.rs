extern crate failure;
#[macro_use]
extern crate failure_derive;
extern crate libc;
#[macro_use]
extern crate log;
#[macro_use]
extern crate nix;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate strum;
#[macro_use]
extern crate strum_macros;

#[cfg(test)]
extern crate env_logger;

pub(crate) mod counter;
pub mod error;
pub mod events;
pub(crate) mod raw;
pub(crate) mod sys;

use std::collections::{BTreeMap, BTreeSet};

use libc::pid_t;

use counter::EventCounter;
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

    pub fn start(&mut self) -> Vec<Result<(), PerfEventsError>> {
        self.counters.iter().map(|c| c.enable()).collect()
    }

    pub fn read(&mut self) -> BTreeMap<Event, u64> {
        self.counters
            .iter_mut()
            .filter_map(|c| {
                let res = c.read();
                if let Err(ref why) = res {
                    debug!("error reading counter: {}", why);
                }
                res.ok()
            })
            .collect()
    }

    pub fn start_all_available() -> Result<Self, PerfEventsError> {
        let res = Counts::new(PidConfig::Current, CpuConfig::All)
            .all_available()
            .create();

        if let (_, Err(ref failures)) = res {
            for (event, error) in failures {
                trace!("error creating event {:?}: {}", event, error);
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
    Specific(i32),
}

impl CpuConfig {
    fn raw(&self) -> i32 {
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
        let _ = env_logger::Builder::from_default_env()
            .filter(None, log::LevelFilter::Debug)
            .try_init();

        let mut counts = Counts::start_all_available().unwrap();
        let first = counts.read();

        trace!("first:\n{:#?}", first);

        for _ in 0..10000 {
            // noop
        }

        let second = counts.read();
        trace!("second:\n{:#?}", second);

        for _ in 0..10000 {
            // noop
        }

        let third = counts.read();
        trace!("third:\n{:#?}", third);

        assert_ne!(first, second);
        assert_ne!(second, third);
        assert_ne!(first, third);

        let first_events = first.iter().map(|e| e.0.to_string()).collect::<Vec<_>>();
        let second_events = second.iter().map(|e| e.0.to_string()).collect::<Vec<_>>();
        let third_events = third.iter().map(|e| e.0.to_string()).collect::<Vec<_>>();

        assert_eq!(first_events, second_events);
        assert_eq!(second_events, third_events);

        debug!("events collected: {:#?}", first_events);
    }
}
