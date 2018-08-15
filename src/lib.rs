// TODO The official way of knowing if perf_event_open() support is enabled
//    is checking for the existence of the file
//    /proc/sys/kernel/perf_event_paranoid.

#[macro_use]
extern crate bitflags;
#[macro_use]
extern crate enum_primitive;
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

extern crate bytes;
extern crate crossbeam_channel as channel;
extern crate futures;
extern crate libc;
extern crate mio;
extern crate mmap;
extern crate num;
extern crate page_size;
extern crate serde;
extern crate strum;
extern crate tokio;

#[cfg(test)]
extern crate env_logger;
#[cfg(test)]
#[macro_use]
extern crate pretty_assertions;
#[cfg(test)]
extern crate rand;

pub(crate) mod count;
pub mod error;
pub(crate) mod fd;
pub(crate) mod raw;
pub mod sample;

use std::collections::{BTreeMap, BTreeSet};

use libc::pid_t;

use count::{CountConfig, Counted, Counter};
pub use error::*;

pub struct Perf {
    counters: Vec<Counter>,
}

impl Perf {
    pub fn new(config: EventConfig) -> PerfBuilder {
        PerfBuilder {
            config,
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
        let res = Perf::new(EventConfig::default())
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
    config: EventConfig,
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
            let config = CountConfig {
                shared: self.config.clone(),
                event: event.clone(),
            };
            match Counter::new(config) {
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord, Serialize)]
pub struct EventConfig {
    pub pid: PidConfig,
    pub cpu: CpuConfig,

    /// If this bit is set, the count excludes events that happen in user space.
    pub exclude_user: bool,

    /// If this bit is set, the count excludes events that happen in kernel space.
    pub exclude_kernel: bool,

    /// If this bit is set, the count excludes events that happen in the hypervisor. This is mainly
    /// for PMUs that have built-in support for handling this (such as POWER). Extra support is
    /// needed for handling hypervisor measurements on most machines.
    pub exclude_hv: bool,

    /// If set, don't count when the CPU is idle.
    pub exclude_idle: bool,

    /// The inherit bit specifies that this counter should count events of child tasks as well as
    /// the task specified. This applies only to new children, not to any existing children at the
    /// time the counter is created (nor to any new children of existing children). Inherit does not
    /// work for some combinations of read_format values, such as PERF_FORMAT_GROUP.
    pub inherit: bool,

    /// This bit enables saving of event counts on context switch for inherited tasks. This is
    /// meaningful only if the inherit field is set.
    pub inherit_stat: bool,

    /// When conducting measurements that include processes running VM instances (i.e., have
    /// executed a KVM_RUN ioctl(2)), only measure events happening inside a guest instance. This is
    /// only meaningful outside the guests; this setting does not change counts gathered inside of a
    /// guest. Currently, this functionality is x86 only. (since Linux 3.2)
    pub exclude_host: bool,

    /// When conducting measurements that include processes running VM instances (i.e., have
    /// executed a KVM_RUN ioctl(2)), do not measure events happening inside guest instances. This
    /// is only meaningful outside the guests; this setting does not change counts gathered inside
    /// of a guest. Currently, this functionality is x86 only. (since Linux 3.2)
    pub exclude_guest: bool,

    /// This allows selecting which internal Linux clock to use when generating timestamps via the
    /// clockid field. This can make it easier to correlate perf sample times with timestamps
    /// generated by other tools.
    ///
    /// If set, then this field selects which internal Linux timer to use for
    /// timestamps. The available timers are defined in linux/time.h, with CLOCK_MONOTONIC,
    /// CLOCK_MONOTONIC_RAW, CLOCK_REALTIME, CLOCK_BOOTTIME, and CLOCK_TAI currently supported.
    ///
    /// (since Linux 4.1)
    pub clockid: Option<i32>,

    /// This specifies how much data is required to trigger a PERF_RECORD_AUX sample. (since Linux
    /// 4.1)
    pub aux_watermark: Option<u32>,
}

impl ::std::default::Default for EventConfig {
    fn default() -> Self {
        EventConfig {
            aux_watermark: None,
            clockid: None,
            exclude_guest: true,
            exclude_host: false,
            inherit_stat: false,
            inherit: false,
            exclude_idle: false,
            exclude_hv: false,
            exclude_kernel: false,
            exclude_user: false,
            pid: PidConfig::Current,
            cpu: CpuConfig::All,
        }
    }
}

use raw::perf_event_attr;

impl EventConfig {
    pub fn raw(&self) -> perf_event_attr {
        use std::mem::{size_of, zeroed};
        let mut attr: perf_event_attr = unsafe { zeroed() };

        attr.set_exclude_user(self.exclude_user as u64);
        attr.set_exclude_kernel(self.exclude_kernel as u64);
        attr.set_exclude_hv(self.exclude_hv as u64);
        attr.set_exclude_idle(self.exclude_idle as u64);
        attr.set_inherit(self.inherit as u64);
        attr.set_inherit_stat(self.inherit_stat as u64);
        attr.set_exclude_host(self.exclude_host as u64);
        attr.set_exclude_guest(self.exclude_guest as u64);

        if let Some(mark) = self.aux_watermark {
            attr.aux_watermark = mark;
        }

        if let Some(clock) = self.clockid {
            attr.set_use_clockid(1);
            attr.clockid = clock;
        }

        attr.size = size_of::<perf_event_attr>() as u32;

        // we start disabled by default, regardless of config
        attr.set_disabled(1);

        attr
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord, Serialize)]
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

impl Default for PidConfig {
    fn default() -> Self {
        PidConfig::Current
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord, Serialize)]
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

impl Default for CpuConfig {
    fn default() -> Self {
        CpuConfig::All
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use env_logger;

    #[test]
    fn test_one_shot() {
        let _ = env_logger::Builder::from_default_env()
            .filter(None, log::LevelFilter::Info)
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
