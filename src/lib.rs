// TODO The official way of knowing if perf_event_open() support is enabled
//    is checking for the existence of the file
//    /proc/sys/kernel/perf_event_paranoid.

#[macro_use]
extern crate bitflags;
#[macro_use]
extern crate enum_primitive;
#[macro_use]
extern crate failure;
#[macro_use]
extern crate failure_derive;
#[macro_use]
extern crate log;
#[macro_use]
extern crate nix;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate strum_macros;

extern crate futures;
extern crate libc;
extern crate mio;
extern crate mmap;
extern crate num;
extern crate page_size;
extern crate serde;
extern crate strum;
extern crate tokio;
extern crate tokio_codec;

#[cfg(test)]
extern crate env_logger;

pub(crate) mod count;
pub mod error;
pub(crate) mod fd;
pub(crate) mod raw;
pub mod sample;

use std::collections::{BTreeMap, BTreeSet};

use libc::pid_t;

use count::{Counted, Counter};
pub use error::*;

pub struct Perf {
    counters: Vec<Counter>,
}

impl Perf {
    pub fn new(pid: PidConfig, cpu: CpuConfig) -> PerfBuilder {
        PerfBuilder {
            pid,
            cpu,
            to_count: BTreeSet::new(),
        }
    }

    pub fn start(&mut self) -> Vec<Result<()>> {
        self.counters.iter().map(|c| c.enable()).collect()
    }

    pub fn read(&mut self) -> BTreeMap<Counted, u64> {
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

    pub fn start_all_counts_available() -> Result<Self> {
        let res = Perf::new(PidConfig::Current, CpuConfig::All)
            .all_counts_available()
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
            Err(Error::Start {
                inner: String::from("No counters started successfully."),
            })
        }
    }
}

#[derive(Debug)]
pub struct PerfBuilder {
    pid: PidConfig,
    cpu: CpuConfig,
    to_count: BTreeSet<Counted>,
}

impl PerfBuilder {
    pub fn all_counts_available(mut self) -> Self {
        for event in Counted::all() {
            self = self.count(event);
        }

        self
    }

    pub fn count(mut self, event: Counted) -> Self {
        self.to_count.insert(event);
        self
    }

    pub fn create(
        self,
    ) -> (
        ::std::result::Result<Perf, ()>,
        ::std::result::Result<(), BTreeMap<Counted, Error>>,
    ) {
        let mut counters = Vec::new();
        let mut failures = BTreeMap::new();

        for event in self.to_count {
            match Counter::new(event.clone(), self.pid, self.cpu) {
                Ok(c) => counters.push(c),
                Err(why) => {
                    failures.insert(event, why);
                }
            };
        }

        let ret_counts = if counters.len() == 0 {
            Err(())
        } else {
            Ok(Perf { counters })
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

        let mut counts = Perf::start_all_counts_available().unwrap();
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
