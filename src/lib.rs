extern crate errno;
extern crate libc;

use std::os::unix::io::RawFd;

use errno::{errno, Errno};
use libc::{c_int, pid_t, syscall, SYS_perf_event_open};

struct Collector {
    group: c_int,
    fds: Vec<c_int>,
    pid: pid_t,
    cpu: c_int,
}

impl Collector {
    pub fn new() -> Self {
        // TODO
        unimplemented!();
    }

    // TODO decide whether to use builder pattern or what
    pub fn counter(self, event: Event) -> Self {
        // TODO
        unimplemented!();
    }
}

fn perf_event_open(
    attr: *const raw::perf_event_attr,
    pid: pid_t,
    cpu: c_int,
    group_fd: c_int,
    flags: u64,
) -> Result<RawFd, Errno> {
    unsafe {
        match syscall(SYS_perf_event_open, attr, pid, cpu, group_fd, flags) {
            -1 => Err(errno()),
            fd => Ok(fd as RawFd),
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
enum Event {
    Hardware(hw::Event),
    Software(sw::Event),
    HardwareCache(hw::cache::Id, hw::cache::OpId, hw::cache::OpResultId),
}

impl Event {
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
}

pub mod sw {
    #[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
    pub enum Event {
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

    impl Event {
        pub(crate) fn config(&self) -> u64 {
            use super::raw::perf_sw_ids::*;
            use Event::*;
            let cfg = match *self {
                CpuClock => PERF_COUNT_SW_CPU_CLOCK,
                TaskClock => PERF_COUNT_SW_TASK_CLOCK,
                PageFaults => PERF_COUNT_SW_PAGE_FAULTS,
                ContextSwitches => PERF_COUNT_SW_CONTEXT_SWITCHES,
                CpuMigrations => PERF_COUNT_SW_CPU_MIGRATIONS,
                PageFaultsMinor => PERF_COUNT_SW_PAGE_FAULTS_MIN,
                PageFaultsMajor => PERF_COUNT_SW_PAGE_FAULTS_MAJ,
                AlignmentFaults => PERF_COUNT_SW_ALIGNMENT_FAULTS,
                EmulationFaults => PERF_COUNT_SW_EMULATION_FAULTS,
            };

            cfg as u64
        }
    }
}
pub mod hw {

    #[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
    pub enum Event {
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

    impl Event {
        pub(crate) fn config(&self) -> u64 {
            use super::raw::perf_hw_id::*;
            use Event::*;
            let cfg = match *self {
                CpuCycles => PERF_COUNT_HW_CPU_CYCLES,
                Instructions => PERF_COUNT_HW_INSTRUCTIONS,
                CacheReferences => PERF_COUNT_HW_CACHE_REFERENCES,
                CacheMisses => PERF_COUNT_HW_CACHE_MISSES,
                BranchInstructions => PERF_COUNT_HW_BRANCH_INSTRUCTIONS,
                BranchMisses => PERF_COUNT_HW_BRANCH_MISSES,
                BusCycles => PERF_COUNT_HW_BUS_CYCLES,
                StalledCyclesFrontend => PERF_COUNT_HW_STALLED_CYCLES_FRONTEND,
                StalledCyclesBackend => PERF_COUNT_HW_STALLED_CYCLES_BACKEND,
                RefCpuCycles => PERF_COUNT_HW_REF_CPU_CYCLES,
            };

            cfg as u64
        }
    }

    pub mod cache {
        #[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
        pub enum Id {
            Level1Data,
            Level1Instruction,
            LastLevel,
            DataTLB,
            InstructionTLB,
            BranchPredictionUnit,
            Node,
        }

        impl Id {
            pub(crate) fn mask(&self) -> u64 {
                use self::Id::*;
                use super::super::raw::perf_hw_cache_id::*;
                let mask = match *self {
                    Level1Data => PERF_COUNT_HW_CACHE_L1D,
                    Level1Instruction => PERF_COUNT_HW_CACHE_L1I,
                    LastLevel => PERF_COUNT_HW_CACHE_LL,
                    DataTLB => PERF_COUNT_HW_CACHE_DTLB,
                    InstructionTLB => PERF_COUNT_HW_CACHE_ITLB,
                    BranchPredictionUnit => PERF_COUNT_HW_CACHE_BPU,
                    Node => PERF_COUNT_HW_CACHE_NODE,
                };

                mask as u64
            }
        }

        #[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
        pub enum OpId {
            Read,
            Write,
            Prefetch,
        }

        impl OpId {
            pub(crate) fn mask(&self) -> u64 {
                use self::OpId::*;
                use raw::perf_hw_cache_op_id::*;
                let mask = match *self {
                    Read => PERF_COUNT_HW_CACHE_OP_READ,
                    Write => PERF_COUNT_HW_CACHE_OP_WRITE,
                    Prefetch => PERF_COUNT_HW_CACHE_OP_PREFETCH,
                };
                mask as u64
            }
        }

        #[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
        pub enum OpResultId {
            Access,
            Miss,
        }

        impl OpResultId {
            pub(crate) fn mask(&self) -> u64 {
                use self::OpResultId::*;
                use raw::perf_hw_cache_op_result_id::*;
                let mask = match *self {
                    Access => PERF_COUNT_HW_CACHE_RESULT_ACCESS,
                    Miss => PERF_COUNT_HW_CACHE_RESULT_MISS,
                };
                mask as u64
            }
        }

    }
}

pub(crate) mod raw {
    #![allow(non_upper_case_globals)]
    #![allow(non_camel_case_types)]
    #![allow(non_snake_case)]

    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}
