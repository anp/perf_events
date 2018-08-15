use std::fmt::{Display, Formatter, Result as FmtResult};
use std::io::prelude::*;
use std::mem::size_of;
use std::slice;

use serde::{Serialize, Serializer};
use strum::IntoEnumIterator;

use raw::perf_hw_cache_id::*;
use raw::perf_hw_cache_op_id::*;
use raw::perf_hw_cache_op_result_id::*;
use raw::perf_hw_id::*;
use raw::perf_sw_ids::*;

use raw::{perf_event_attr, perf_type_id};

use super::{CpuConfig, EventConfig, PidConfig};
use error::*;
use fd::{PerfEventAttrThingy, PerfFile};

#[derive(Debug)]
pub struct Counter {
    config: CountConfig,
    file: PerfFile,
}

#[derive(Clone, Copy, Debug)]
pub struct CountConfig {
    pub event: Counted,
    pub shared: EventConfig,
}

impl Into<::raw::perf_event_attr> for CountConfig {
    fn into(self) -> ::raw::perf_event_attr {
        let mut attr = self.shared.raw();
        self.event.apply(&mut attr);
        attr
    }
}

impl AsRef<CpuConfig> for CountConfig {
    fn as_ref(&self) -> &CpuConfig {
        &self.shared.cpu
    }
}

impl AsRef<PidConfig> for CountConfig {
    fn as_ref(&self) -> &PidConfig {
        &self.shared.pid
    }
}

impl Counter {
    pub fn new(config: CountConfig) -> Result<Self> {
        let file = PerfFile::new(config)?;
        Ok(Self { config, file })
    }

    pub fn enable(&self) -> Result<()> {
        self.file.enable()
    }

    pub fn read(&mut self) -> Result<(Counted, u64)> {
        let mut value: u64 = 0;

        // NOTE(unsafe): we're just generating a pointer to a stack variable,
        // not saving that pointer beyond this stack frame
        let mut value_slice = unsafe {
            let ptr = (&mut value as *mut u64) as *mut u8;
            let len = size_of::<u64>();
            slice::from_raw_parts_mut(ptr, len)
        };

        self.file.read(&mut value_slice)?;

        Ok((self.config.event.clone(), value))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord, Serialize)]
#[serde(untagged)]
pub enum Counted {
    Hardware(HwEvent),
    Software(SwEvent),
    HardwareCache(HardwareCacheSpec),
}

impl PerfEventAttrThingy for Counted {
    fn apply(&self, attr: &mut perf_event_attr) {
        let (ty, config) = match *self {
            Counted::Hardware(hw_id) => (perf_type_id::PERF_TYPE_HARDWARE, hw_id as u64),
            Counted::Software(sw_id) => (perf_type_id::PERF_TYPE_SOFTWARE, sw_id as u64),
            Counted::HardwareCache(HardwareCacheSpec(id, op_id, op_result_id)) => (
                perf_type_id::PERF_TYPE_HW_CACHE,
                id as u64 | (op_id as u64) << 8 | (op_result_id as u64) << 16,
            ),
        };

        attr.type_ = ty;
        attr.config = config;
    }
}

impl Counted {
    pub(crate) fn all() -> Vec<Self> {
        let mut variants = Vec::new();

        for hw_event in HwEvent::iter() {
            variants.push(Counted::Hardware(hw_event));
        }

        for sw_event in SwEvent::iter() {
            // this can be specially requested
            if sw_event == SwEvent::DummyForSampled {
                continue;
            }

            variants.push(Counted::Software(sw_event));
        }

        for cache_id in CacheId::iter() {
            for cache_op_id in CacheOpId::iter() {
                for cache_op_result_id in CacheOpResultId::iter() {
                    variants.push(Counted::HardwareCache(HardwareCacheSpec(
                        cache_id,
                        cache_op_id,
                        cache_op_result_id,
                    )))
                }
            }
        }

        variants
    }
}

impl Display for Counted {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        match self {
            Counted::Hardware(hwe) => f.write_fmt(format_args!("Hardware: {}", hwe)),
            Counted::Software(swe) => f.write_fmt(format_args!("Software: {}", swe)),
            Counted::HardwareCache(spec) => f.write_str("Cache: ").and_then(|()| spec.fmt(f)),
        }
    }
}

#[repr(u64)]
#[derive(Clone, Copy, Debug, Display, EnumIter, Eq, PartialEq, PartialOrd, Ord, Serialize)]
pub enum SwEvent {
    /// This reports the CPU clock, a high-resolution per-CPU timer.
    #[serde(rename = "cpu-clock")]
    #[strum(to_string = "CPU Clock")]
    CpuClock = PERF_COUNT_SW_CPU_CLOCK as u64,

    /// This reports a clock count specific to the task that is running.
    #[serde(rename = "task-clock")]
    #[strum(to_string = "Task Clock")]
    TaskClock = PERF_COUNT_SW_TASK_CLOCK as u64,

    /// This counts context switches. Until Linux 2.6.34, these were all
    /// reported as user-space events, after that they are reported as
    /// happening in the kernel.
    #[serde(rename = "context-switches")]
    #[strum(to_string = "Context Switches")]
    ContextSwitches = PERF_COUNT_SW_CONTEXT_SWITCHES as u64,

    /// This reports the number of times the process has migrated to a new CPU.
    #[serde(rename = "cpu-migrations")]
    #[strum(to_string = "CPU Migrations")]
    CpuMigrations = PERF_COUNT_SW_CPU_MIGRATIONS as u64,

    /// This reports the number of page faults.
    #[serde(rename = "page-fault")]
    #[strum(to_string = "Page Faults")]
    PageFaults = PERF_COUNT_SW_PAGE_FAULTS as u64,

    /// This counts the number of minor page faults. These did not require disk
    /// I/O to handle.
    #[serde(rename = "page-fault-minor")]
    #[strum(to_string = "Page Faults, Minor")]
    PageFaultsMinor = PERF_COUNT_SW_PAGE_FAULTS_MIN as u64,

    /// This counts the number of major page faults. These required disk I/O
    /// to handle.
    #[serde(rename = "page-faults-major")]
    #[strum(to_string = "Page Faults, Major")]
    PageFaultsMajor = PERF_COUNT_SW_PAGE_FAULTS_MAJ as u64,

    /// This counts the number of alignment faults. These happen when
    /// unaligned memory accesses happen; the kernel can handle these but it
    /// reduces performance. This happens only on some architectures (never on
    /// x86).
    ///
    /// (since Linux 2.6.33)
    #[serde(rename = "align-faults")]
    #[strum(to_string = "Alignment Faults")]
    AlignmentFaults = PERF_COUNT_SW_ALIGNMENT_FAULTS as u64,

    /// This counts the number of emulation faults. The kernel somtimes traps
    /// on unimplemented instructions and emulates them for user space. This
    /// can negatively impact performance.
    ///
    /// (since Linux 2.6.33)
    #[serde(rename = "emulation-faults")]
    #[strum(to_string = "Emulation Faults")]
    EmulationFaults = PERF_COUNT_SW_EMULATION_FAULTS as u64,

    /// This is a placeholder event that counts nothing. Informational sample record types such as
    /// mmap or comm must be associated with an active event. This dummy event allows gathering such
    /// records without requiring a counting event.
    ///
    /// (since Linux 3.12)
    #[serde(rename = "dummy")]
    #[strum(to_string = "Dummy (for sampled metrics)")]
    DummyForSampled = PERF_COUNT_SW_DUMMY as u64,
}

#[repr(u64)]
#[derive(Clone, Copy, Debug, Display, EnumIter, Eq, PartialEq, PartialOrd, Ord, Serialize)]
pub enum HwEvent {
    /// Total cycles. Be wary of what happens during CPU frequency scaling.
    #[serde(rename = "cpu-cycles")]
    #[strum(to_string = "CPU Cycles")]
    CpuCycles = PERF_COUNT_HW_CPU_CYCLES as u64,

    /// Retired instructions. Be careful, these can be affected by various
    /// issues, most notably hardware interrupt counts.
    #[serde(rename = "instructions")]
    Instructions = PERF_COUNT_HW_INSTRUCTIONS as u64,

    /// Cache accesses. Usually this indicates Last Level Cache accesses but
    /// this may vary depending on your CPU. This may include prefetches and
    /// coherency messages; again this depends on the design of your CPU.
    #[serde(rename = "cache-references")]
    #[strum(to_string = "Cache References")]
    CacheReferences = PERF_COUNT_HW_CACHE_REFERENCES as u64,

    /// Cache misses. Usually this indicates Last Level Cache misses;
    /// this is intended to be used in conjunction with the
    /// `CacheReferences` event to calculate cache miss rates.
    #[serde(rename = "cache-misses")]
    #[strum(to_string = "Cache Misses")]
    CacheMisses = PERF_COUNT_HW_CACHE_MISSES as u64,

    /// Retired branch instructions. Prior to Linux 2.6.35, this used
    /// the wrong event on AMD processors.
    #[serde(rename = "branch-instructions")]
    #[strum(to_string = "Branch Instructions")]
    BranchInstructions = PERF_COUNT_HW_BRANCH_INSTRUCTIONS as u64,

    /// Mispredicted branch instructions.
    #[serde(rename = "branch-misses")]
    #[strum(to_string = "Branch Misses")]
    BranchMisses = PERF_COUNT_HW_BRANCH_MISSES as u64,

    /// Bus cycles, which can be different from total cycles.
    #[serde(rename = "bus-cycles")]
    #[strum(to_string = "Bus Cycles")]
    BusCycles = PERF_COUNT_HW_BUS_CYCLES as u64,

    /// Stalled cycles during issue. (since Linux 3.0)
    #[serde(rename = "stalled-cycles-frontend")]
    #[strum(to_string = "Stalled Cycles, Frontend")]
    StalledCyclesFrontend = PERF_COUNT_HW_STALLED_CYCLES_FRONTEND as u64,

    /// Stalled cycles during retirement. (since Linux 3.0)
    #[serde(rename = "stalled-cycles-backend")]
    #[strum(to_string = "Stalled Cycles, Backend")]
    StalledCyclesBackend = PERF_COUNT_HW_STALLED_CYCLES_BACKEND as u64,

    /// Total cycles; not affected by CPU frequency scaling. (since Linux 3.3)
    #[serde(rename = "ref-cpu-cycles")]
    #[strum(to_string = "Total CPU Cycles")]
    RefCpuCycles = PERF_COUNT_HW_REF_CPU_CYCLES as u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub struct HardwareCacheSpec(CacheId, CacheOpId, CacheOpResultId);

impl Serialize for HardwareCacheSpec {
    fn serialize<S>(&self, serializer: S) -> ::std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(&format_args!(
            "{}-{}-{}",
            self.0.str(),
            self.1.str(),
            self.2.str()
        ))
    }
}

impl Display for HardwareCacheSpec {
    fn fmt(&self, f: &mut Formatter) -> ::std::fmt::Result {
        f.write_fmt(format_args!("{} {} {}", self.0, self.1, self.2))
    }
}

/// perf_hw_cache_id wrapper.
#[repr(u64)]
#[derive(Clone, Copy, Debug, Display, EnumIter, Eq, PartialEq, PartialOrd, Ord)]
pub enum CacheId {
    /// for measuring Level 1 Data Cache
    #[strum(to_string = "Level 1 Data Cache")]
    Level1Data = PERF_COUNT_HW_CACHE_L1D as u64,

    /// for measuring Level 1 Instruction Cache
    #[strum(to_string = "Level 1 Instruction Cache")]
    Level1Instruction = PERF_COUNT_HW_CACHE_L1I as u64,

    /// for measuring Last-Level Cache
    #[strum(to_string = "Last-Level Cache")]
    LastLevel = PERF_COUNT_HW_CACHE_LL as u64,

    /// for measuring the Data TLB
    #[strum(to_string = "Data TLB")]
    DataTLB = PERF_COUNT_HW_CACHE_DTLB as u64,

    /// for measuring the Instruction TLB
    #[strum(to_string = "Instruction TLB")]
    InstructionTLB = PERF_COUNT_HW_CACHE_ITLB as u64,

    /// for measuring the branch prediction unit
    #[strum(to_string = "Branch Prediction Unit")]
    BranchPredictionUnit = PERF_COUNT_HW_CACHE_BPU as u64,

    /// for measuring local memory accesses (since Linux 3.1)
    #[strum(to_string = "Local Memory Accesses")]
    Node = PERF_COUNT_HW_CACHE_NODE as u64,
}

impl CacheId {
    fn str(&self) -> &'static str {
        match *self {
            CacheId::Level1Data => "l1d",
            CacheId::Level1Instruction => "l1i",
            CacheId::LastLevel => "ll",
            CacheId::DataTLB => "dtlb",
            CacheId::InstructionTLB => "itlb",
            CacheId::BranchPredictionUnit => "bpu",
            CacheId::Node => "node",
        }
    }
}

/// perf_hw_cache_op_id wrapper.
#[repr(u64)]
#[derive(Clone, Copy, Debug, Display, EnumIter, Eq, PartialEq, PartialOrd, Ord)]
pub enum CacheOpId {
    /// for read accesses
    Read = PERF_COUNT_HW_CACHE_OP_READ as u64,
    /// for write accesses
    Write = PERF_COUNT_HW_CACHE_OP_WRITE as u64,
    /// for prefetch accesses
    Prefetch = PERF_COUNT_HW_CACHE_OP_PREFETCH as u64,
}

impl CacheOpId {
    fn str(&self) -> &'static str {
        match *self {
            CacheOpId::Read => "read",
            CacheOpId::Write => "write",
            CacheOpId::Prefetch => "prefetch",
        }
    }
}

/// perf_hw_cache_op_result_id wrapper.
#[repr(u64)]
#[derive(Clone, Copy, Debug, Display, EnumIter, Eq, PartialEq, PartialOrd, Ord)]
pub enum CacheOpResultId {
    /// to measure accesses
    Access = PERF_COUNT_HW_CACHE_RESULT_ACCESS as u64,
    /// to measure misses
    Miss = PERF_COUNT_HW_CACHE_RESULT_MISS as u64,
}

impl CacheOpResultId {
    fn str(&self) -> &'static str {
        match *self {
            CacheOpResultId::Access => "access",
            CacheOpResultId::Miss => "miss",
        }
    }
}
