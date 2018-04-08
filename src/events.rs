use std::mem::{size_of, zeroed};

use strum::IntoEnumIterator;

use raw::perf_hw_cache_id::*;
use raw::perf_hw_cache_op_id::*;
use raw::perf_hw_cache_op_result_id::*;
use raw::perf_hw_id::*;
use raw::perf_sw_ids::*;

use raw::{perf_event_attr, perf_type_id};

#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub enum Event {
    Hardware(HwEvent),
    Software(SwEvent),
    HardwareCache(CacheId, CacheOpId, CacheOpResultId),
}

impl Event {
    pub(crate) fn all_events() -> Vec<Event> {
        let mut variants = Vec::new();

        for hw_event in HwEvent::iter() {
            variants.push(Event::Hardware(hw_event));
        }

        for sw_event in SwEvent::iter() {
            variants.push(Event::Software(sw_event));
        }

        for cache_id in CacheId::iter() {
            for cache_op_id in CacheOpId::iter() {
                for cache_op_result_id in CacheOpResultId::iter() {
                    variants.push(Event::HardwareCache(
                        cache_id,
                        cache_op_id,
                        cache_op_result_id,
                    ))
                }
            }
        }

        variants
    }

    fn type_(&self) -> perf_type_id {
        match *self {
            Event::Hardware(_) => perf_type_id::PERF_TYPE_HARDWARE,
            Event::Software(_) => perf_type_id::PERF_TYPE_SOFTWARE,
            Event::HardwareCache(_, _, _) => perf_type_id::PERF_TYPE_HW_CACHE,
        }
    }

    fn config(&self) -> u64 {
        match *self {
            Event::Hardware(hw_id) => hw_id as u64,
            Event::Software(sw_id) => sw_id as u64,
            Event::HardwareCache(id, op_id, op_result_id) => {
                id as u64 | (op_id as u64) << 8 | (op_result_id as u64) << 16
            }
        }
    }

    pub(crate) fn as_raw(&self, disabled: bool) -> perf_event_attr {
        // NOTE(unsafe) a zeroed struct is what the example c code uses,
        // zero fields are interpreted as "off" afaict, aside from the required fields
        let mut raw_event: perf_event_attr = unsafe { zeroed() };

        raw_event.type_ = self.type_() as u32;
        raw_event.size = size_of::<perf_event_attr>() as u32;
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

#[repr(u64)]
#[derive(Clone, Copy, Debug, EnumIter, Eq, PartialEq, PartialOrd, Ord)]
pub enum SwEvent {
    CpuClock = PERF_COUNT_SW_CPU_CLOCK as u64,
    TaskClock = PERF_COUNT_SW_TASK_CLOCK as u64,
    PageFaults = PERF_COUNT_SW_PAGE_FAULTS as u64,
    ContextSwitches = PERF_COUNT_SW_CONTEXT_SWITCHES as u64,
    CpuMigrations = PERF_COUNT_SW_CPU_MIGRATIONS as u64,
    PageFaultsMinor = PERF_COUNT_SW_PAGE_FAULTS_MIN as u64,
    PageFaultsMajor = PERF_COUNT_SW_PAGE_FAULTS_MAJ as u64,
    AlignmentFaults = PERF_COUNT_SW_ALIGNMENT_FAULTS as u64,
    EmulationFaults = PERF_COUNT_SW_EMULATION_FAULTS as u64,
}

#[repr(u64)]
#[derive(Clone, Copy, Debug, EnumIter, Eq, PartialEq, PartialOrd, Ord)]
pub enum HwEvent {
    CpuCycles = PERF_COUNT_HW_CPU_CYCLES as u64,
    Instructions = PERF_COUNT_HW_INSTRUCTIONS as u64,
    CacheReferences = PERF_COUNT_HW_CACHE_REFERENCES as u64,
    CacheMisses = PERF_COUNT_HW_CACHE_MISSES as u64,
    BranchInstructions = PERF_COUNT_HW_BRANCH_INSTRUCTIONS as u64,
    BranchMisses = PERF_COUNT_HW_BRANCH_MISSES as u64,
    BusCycles = PERF_COUNT_HW_BUS_CYCLES as u64,
    StalledCyclesFrontend = PERF_COUNT_HW_STALLED_CYCLES_FRONTEND as u64,
    StalledCyclesBackend = PERF_COUNT_HW_STALLED_CYCLES_BACKEND as u64,
    RefCpuCycles = PERF_COUNT_HW_REF_CPU_CYCLES as u64,
}

#[repr(u64)]
#[derive(Clone, Copy, Debug, EnumIter, Eq, PartialEq, PartialOrd, Ord)]
pub enum CacheId {
    Level1Data = PERF_COUNT_HW_CACHE_L1D as u64,
    Level1Instruction = PERF_COUNT_HW_CACHE_L1I as u64,
    LastLevel = PERF_COUNT_HW_CACHE_LL as u64,
    DataTLB = PERF_COUNT_HW_CACHE_DTLB as u64,
    InstructionTLB = PERF_COUNT_HW_CACHE_ITLB as u64,
    BranchPredictionUnit = PERF_COUNT_HW_CACHE_BPU as u64,
    Node = PERF_COUNT_HW_CACHE_NODE as u64,
}

#[repr(u64)]
#[derive(Clone, Copy, Debug, EnumIter, Eq, PartialEq, PartialOrd, Ord)]
pub enum CacheOpId {
    Read = PERF_COUNT_HW_CACHE_OP_READ as u64,
    Write = PERF_COUNT_HW_CACHE_OP_WRITE as u64,
    Prefetch = PERF_COUNT_HW_CACHE_OP_PREFETCH as u64,
}

#[repr(u64)]
#[derive(Clone, Copy, Debug, EnumIter, Eq, PartialEq, PartialOrd, Ord)]
pub enum CacheOpResultId {
    Access = PERF_COUNT_HW_CACHE_RESULT_ACCESS as u64,
    Miss = PERF_COUNT_HW_CACHE_RESULT_MISS as u64,
}
