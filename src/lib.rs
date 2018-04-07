#[macro_use]
extern crate bitflags;
extern crate errno;
extern crate libc;

// TODO logging
// TODO better error handling

use std::collections::BTreeMap;
use std::io;
use std::os::unix::io::RawFd;

use errno::{errno, Errno};
use libc::{c_int, c_ulong, pid_t, syscall, SYS_perf_event_open};

pub struct Counts {}

impl Counts {
    pub fn new(pid: PidConfig, cpu: CpuConfig) -> CountsBuilder {
        // TODO
        unimplemented!();
    }

    pub fn read(&mut self) -> io::Result<BTreeMap<EventCounter, u64>> {
        unimplemented!();
    }
}

pub struct CountsBuilder {
    pid: PidConfig,
    cpu: CpuConfig,
    counting: bool,
    group_fd: Option<RawFd>,
    flags: Flags,
}

impl CountsBuilder {
    pub fn count_all_available(self) {
        // TODO
        unimplemented!();
    }

    // TODO decide whether to use builder pattern or what
    pub fn add_event_counter(mut self, event: EventCounter) -> Result<(), Errno> {
        let raw = event.as_raw();

        let group_fd = match self.group_fd {
            Some(f) => f,
            None => -1,
        };

        let ret_fd = perf_event_open(
            &raw,
            self.pid.raw(),
            self.cpu.raw(),
            group_fd,
            self.flags.bits,
        )?;

        self.group_fd = Some(ret_fd);

        Ok(())
    }

    pub fn start(self) -> Result<Counts, String> {
        // TODO ioctl enable
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

bitflags! {
    struct Flags: c_ulong {
       const FD_CLOEXEC = raw::PERF_FLAG_FD_CLOEXEC as c_ulong;
       const FD_NO_GROUP = raw::PERF_FLAG_FD_NO_GROUP as c_ulong;
       const FD_OUTPUT = raw::PERF_FLAG_FD_OUTPUT as c_ulong;
       const PID_CGROUP = raw::PERF_FLAG_PID_CGROUP as c_ulong;
    }
}

fn perf_event_open(
    attr: *const raw::perf_event_attr,
    pid: pid_t,
    cpu: c_int,
    group_fd: c_int,
    flags: c_ulong,
) -> Result<RawFd, Errno> {
    unsafe {
        match syscall(SYS_perf_event_open, attr, pid, cpu, group_fd, flags) {
            -1 => Err(errno()),
            fd => Ok(fd as RawFd),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub enum EventCounter {
    Hardware(HwEvent),
    Software(SwEvent),
    HardwareCache(CacheId, CacheOpId, CacheOpResultId),
}

impl EventCounter {
    pub fn available(&self) -> Result<(), ()> {
        // TODO
        unimplemented!();
    }

    fn type_(&self) -> raw::perf_type_id {
        use raw::perf_type_id::*;
        match *self {
            EventCounter::Hardware(_) => PERF_TYPE_HARDWARE,
            EventCounter::Software(_) => PERF_TYPE_SOFTWARE,
            EventCounter::HardwareCache(_, _, _) => PERF_TYPE_HW_CACHE,
        }
    }

    fn config(&self) -> u64 {
        match *self {
            EventCounter::Hardware(hw_id) => hw_id.config(),
            EventCounter::Software(sw_id) => sw_id.config(),
            EventCounter::HardwareCache(id, op_id, op_result_id) => {
                id.mask() | (op_id.mask() << 8) | (op_result_id.mask() << 16)
            }
        }
    }

    fn as_raw(&self) -> raw::perf_event_attr {
        unimplemented!();
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
    pub(crate) fn mask(&self) -> u64 {
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
    pub(crate) fn mask(&self) -> u64 {
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
    pub(crate) fn mask(&self) -> u64 {
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
