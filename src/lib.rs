extern crate errno;
extern crate libc;

// TODO logging
// TODO better error handling

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io;
use std::os::unix::io::RawFd;

use errno::{errno, Errno};
use libc::{c_int, pid_t, syscall, SYS_perf_event_open};

pub struct Counts {}

impl Counts {
    pub fn new(pid: PidConfig, cpu: CpuConfig) -> CountsBuilder {
        // TODO
        unimplemented!();
    }

    // TODO ioctl enable
}

pub struct CountsBuilder {
    pid: PidConfig,
    cpu: CpuConfig,
    counting: bool,
    to_count: BTreeSet<Event>,
}

impl CountsBuilder {
    pub fn all_available(self) {
        // TODO
        unimplemented!();
    }

    pub fn event(mut self, event: Event) -> Self {
        self.to_count.insert(event);
        self
    }

    pub fn init(self) -> (Result<Counts, ()>, Result<(), BTreeMap<Event, Errno>>) {
        unimplemented!();
    }
}

pub enum PidConfig {
    Current,
    Other(pid_t),
}

impl PidConfig {
    fn raw(&self) -> pid_t {
        match *self {
            PidConfig::Current => -1,
            PidConfig::Other(p) => p,
        }
    }
}

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

struct EventCounter {
    event: Event,
    file: File,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub enum Event {
    Hardware(HwEvent),
    Software(SwEvent),
    HardwareCache(CacheId, CacheOpId, CacheOpResultId),
}

impl Event {
    fn create_fd(&self, pid: pid_t, cpu: c_int) -> Result<RawFd, Errno> {
        unsafe {
            match syscall(
                SYS_perf_event_open,
                &self.as_raw(true),
                pid,
                cpu,
                // ignore group_fd, since we can't set inherit *and* read multiple from a group
                -1,
                // NOTE: doesnt seem like this is needed for this library, but
                // i could be wrong. CLOEXEC doesn't seem to apply when we won't
                // leak the file descriptor, NO_GROUP doesn't make since FD_OUTPUT
                // has been broken since 2.6.35, and PID_CGROUP isn't useful
                // unless you're running inside containers, which i don't need to
                // support yet
                0,
            ) {
                -1 => Err(errno()),
                fd => Ok(fd as RawFd),
            }
        }
    }

    fn type_(&self) -> raw::perf_type_id {
        use raw::perf_type_id::*;
        match *self {
            Event::Hardware(_) => PERF_TYPE_HARDWARE,
            Event::Software(_) => PERF_TYPE_SOFTWARE,
            Event::HardwareCache(_, _, _) => PERF_TYPE_HW_CACHE,
        }
    }

    fn config(&self) -> u64 {
        match *self {
            Event::Hardware(hw_id) => hw_id.config(),
            Event::Software(sw_id) => sw_id.config(),
            Event::HardwareCache(id, op_id, op_result_id) => {
                id.mask() | (op_id.mask() << 8) | (op_result_id.mask() << 16)
            }
        }
    }

    fn as_raw(&self, disabled: bool) -> raw::perf_event_attr {
        let mut raw_event: raw::perf_event_attr = unsafe { std::mem::zeroed() };

        raw_event.type_ = self.type_() as u32;
        raw_event.size = std::mem::size_of::<raw::perf_event_attr>() as u32;
        raw_event.config = self.config();

        // TODO decide whether to change the read format
        if disabled {
            raw_event.set_disabled(1);
        }

        // make sure any threads spawned after starting to count are included
        raw_event.set_inherit(1);
        // TODO maybe figure out what inherit_stat actually does?

        raw_event
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub enum SwEvent {
    CpuClock,
    TaskClock,
    PageFaults,
    ContextSwitches,
    CpuMigrations,
    PageFaultsMinor,
    PageFaultsMajor,
    AlignmentFaults,
    EmulationFaults,
}

impl SwEvent {
    pub(crate) fn config(&self) -> u64 {
        use raw::perf_sw_ids::*;
        let cfg = match *self {
            SwEvent::CpuClock => PERF_COUNT_SW_CPU_CLOCK,
            SwEvent::TaskClock => PERF_COUNT_SW_TASK_CLOCK,
            SwEvent::PageFaults => PERF_COUNT_SW_PAGE_FAULTS,
            SwEvent::ContextSwitches => PERF_COUNT_SW_CONTEXT_SWITCHES,
            SwEvent::CpuMigrations => PERF_COUNT_SW_CPU_MIGRATIONS,
            SwEvent::PageFaultsMinor => PERF_COUNT_SW_PAGE_FAULTS_MIN,
            SwEvent::PageFaultsMajor => PERF_COUNT_SW_PAGE_FAULTS_MAJ,
            SwEvent::AlignmentFaults => PERF_COUNT_SW_ALIGNMENT_FAULTS,
            SwEvent::EmulationFaults => PERF_COUNT_SW_EMULATION_FAULTS,
        };

        cfg as u64
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub enum HwEvent {
    CpuCycles,
    Instructions,
    CacheReferences,
    CacheMisses,
    BranchInstructions,
    BranchMisses,
    BusCycles,
    StalledCyclesFrontend,
    StalledCyclesBackend,
    RefCpuCycles,
}

impl HwEvent {
    pub(crate) fn config(&self) -> u64 {
        use raw::perf_hw_id::*;
        let cfg = match *self {
            HwEvent::CpuCycles => PERF_COUNT_HW_CPU_CYCLES,
            HwEvent::Instructions => PERF_COUNT_HW_INSTRUCTIONS,
            HwEvent::CacheReferences => PERF_COUNT_HW_CACHE_REFERENCES,
            HwEvent::CacheMisses => PERF_COUNT_HW_CACHE_MISSES,
            HwEvent::BranchInstructions => PERF_COUNT_HW_BRANCH_INSTRUCTIONS,
            HwEvent::BranchMisses => PERF_COUNT_HW_BRANCH_MISSES,
            HwEvent::BusCycles => PERF_COUNT_HW_BUS_CYCLES,
            HwEvent::StalledCyclesFrontend => PERF_COUNT_HW_STALLED_CYCLES_FRONTEND,
            HwEvent::StalledCyclesBackend => PERF_COUNT_HW_STALLED_CYCLES_BACKEND,
            HwEvent::RefCpuCycles => PERF_COUNT_HW_REF_CPU_CYCLES,
        };

        cfg as u64
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub enum CacheId {
    Level1Data,
    Level1Instruction,
    LastLevel,
    DataTLB,
    InstructionTLB,
    BranchPredictionUnit,
    Node,
}

impl CacheId {
    fn mask(&self) -> u64 {
        use raw::perf_hw_cache_id::*;
        let mask = match *self {
            CacheId::Level1Data => PERF_COUNT_HW_CACHE_L1D,
            CacheId::Level1Instruction => PERF_COUNT_HW_CACHE_L1I,
            CacheId::LastLevel => PERF_COUNT_HW_CACHE_LL,
            CacheId::DataTLB => PERF_COUNT_HW_CACHE_DTLB,
            CacheId::InstructionTLB => PERF_COUNT_HW_CACHE_ITLB,
            CacheId::BranchPredictionUnit => PERF_COUNT_HW_CACHE_BPU,
            CacheId::Node => PERF_COUNT_HW_CACHE_NODE,
        };

        mask as u64
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub enum CacheOpId {
    Read,
    Write,
    Prefetch,
}

impl CacheOpId {
    fn mask(&self) -> u64 {
        use raw::perf_hw_cache_op_id::*;
        let mask = match *self {
            CacheOpId::Read => PERF_COUNT_HW_CACHE_OP_READ,
            CacheOpId::Write => PERF_COUNT_HW_CACHE_OP_WRITE,
            CacheOpId::Prefetch => PERF_COUNT_HW_CACHE_OP_PREFETCH,
        };
        mask as u64
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub enum CacheOpResultId {
    Access,
    Miss,
}

impl CacheOpResultId {
    fn mask(&self) -> u64 {
        use raw::perf_hw_cache_op_result_id::*;
        let mask = match *self {
            CacheOpResultId::Access => PERF_COUNT_HW_CACHE_RESULT_ACCESS,
            CacheOpResultId::Miss => PERF_COUNT_HW_CACHE_RESULT_MISS,
        };
        mask as u64
    }
}

pub mod raw {
    #![allow(non_upper_case_globals)]
    #![allow(non_camel_case_types)]
    #![allow(non_snake_case)]

    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}
