use std::fmt::{Display, Error, Formatter};
use std::mem::{size_of, zeroed};

use failure::err_msg;
use serde::{Serialize, Serializer};
use strum::IntoEnumIterator;

use raw::perf_hw_cache_id::*;
use raw::perf_hw_cache_op_id::*;
use raw::perf_hw_cache_op_result_id::*;
use raw::perf_hw_id::*;
use raw::perf_sw_ids::*;

use raw::{perf_event_attr, perf_type_id};

#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord, Serialize)]
#[serde(untagged)]
pub enum Event {
    Hardware(HwEvent),
    Software(SwEvent),
    HardwareCache(HardwareCacheSpec),
    Sampled(SampledEventSpec),
}

impl Event {
    pub(crate) fn all_counted_events() -> Vec<Self> {
        let mut variants = Vec::new();

        for hw_event in HwEvent::iter() {
            variants.push(Event::Hardware(hw_event));
        }

        for sw_event in SwEvent::iter() {
            // this can be specially requested
            if sw_event == SwEvent::DummyForSampled {
                continue;
            }

            variants.push(Event::Software(sw_event));
        }

        for cache_id in CacheId::iter() {
            for cache_op_id in CacheOpId::iter() {
                for cache_op_result_id in CacheOpResultId::iter() {
                    variants.push(Event::HardwareCache(HardwareCacheSpec(
                        cache_id,
                        cache_op_id,
                        cache_op_result_id,
                    )))
                }
            }
        }

        variants
    }

    fn type_(&self) -> u32 {
        match *self {
            Event::Hardware(_) => perf_type_id::PERF_TYPE_HARDWARE,
            Event::Software(_) => perf_type_id::PERF_TYPE_SOFTWARE,
            Event::HardwareCache(_) => perf_type_id::PERF_TYPE_HW_CACHE,
            Event::Sampled(_) => perf_type_id::PERF_TYPE_SOFTWARE,
        }
    }

    fn config(&self) -> u64 {
        match *self {
            Event::Hardware(hw_id) => hw_id as u64,
            Event::Software(sw_id) => sw_id as u64,
            Event::HardwareCache(HardwareCacheSpec(id, op_id, op_result_id)) => {
                id as u64 | (op_id as u64) << 8 | (op_result_id as u64) << 16
            }
            Event::Sampled(_) => SwEvent::DummyForSampled as u64,
        }
    }

    fn apply(&self, attr: &mut perf_event_attr) {
        attr.type_ = self.type_();
        attr.config = self.config();

        match *self {
            Event::Sampled(spec) => {
                spec.apply(attr);
            }
            _ => (),
        }
    }

    pub(crate) fn as_raw(&self, disabled: bool) -> perf_event_attr {
        // NOTE(unsafe) a zeroed struct is what the example c code uses,
        // zero fields are interpreted as "off" afaict, aside from the required fields
        let mut raw_event: perf_event_attr = unsafe { zeroed() };

        self.apply(&mut raw_event);

        raw_event.size = size_of::<perf_event_attr>() as u32;

        // from the linux manpage example
        raw_event.set_exclude_kernel(1);
        raw_event.set_exclude_hv(1);

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

impl Display for Event {
    fn fmt(&self, f: &mut Formatter) -> Result<(), Error> {
        match *self {
            Event::Hardware(hwe) => f.write_fmt(format_args!("Hardware: {}", hwe)),
            Event::Software(swe) => f.write_fmt(format_args!("Software: {}", swe)),
            Event::HardwareCache(spec) => f.write_str("Cache: ").and_then(|()| spec.fmt(f)),
            Event::Sampled(sample_spec) => f
                .write_str("Sampled: ")
                .and_then(|()| f.write_fmt(format_args!("{:?}", sample_spec))),
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
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord, Serialize)]
pub struct SampledEventSpec {
    rate: SamplingRate,
    ty: SamplingType,
    /// If set, then TID, TIME, ID, STREAM_ID, and CPU can additionally be included in
    /// non-PERF_RECORD_SAMPLEs if the corresponding sample_type is selected. (since Linux 2.6.38)
    ///
    /// If PERF_SAMPLE_IDENTIFIER is specified, then an additional ID value is included as the last
    /// value to ease parsing the record stream. This may lead to the id value appearing twice.
    sample_id_all: bool,
    wakeup: WakeupConfig,
    //    sample_regs_user (since Linux 3.7)
    //           This bit mask defines the set of user CPU registers to dump on
    //           samples.  The layout of the register mask is architecture-spe‐
    //           cific and is described in the kernel header file
    //           arch/ARCH/include/uapi/asm/perf_regs.h.

    //    sample_stack_user (since Linux 3.7)
    //           This defines the size of the user stack to dump if PERF_SAM‐
    //           PLE_STACK_USER is specified.

    //    clockid (since Linux 4.1)
    //           If use_clockid is set, then this field selects which internal
    //           Linux timer to use for timestamps.  The available timers are
    //           defined in linux/time.h, with CLOCK_MONOTONIC, CLOCK_MONO‐
    //           TONIC_RAW, CLOCK_REALTIME, CLOCK_BOOTTIME, and CLOCK_TAI cur‐
    //           rently supported.

    //    aux_watermark (since Linux 4.1)
    //           This specifies how much data is required to trigger a
    //           PERF_RECORD_AUX sample.

    // TODO(anp):
    //    sample_max_stack (since Linux 4.8)
    //           When sample_type includes PERF_SAMPLE_CALLCHAIN, this field
    //           specifies how many stack frames to report when generating the
    //           callchain.
}

impl SampledEventSpec {
    fn apply(&self, attr: &mut perf_event_attr) {
        self.rate.apply(attr);
        self.wakeup.apply(attr);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord, Serialize)]
pub enum SamplingRate {
    /// A "sampling" event is one that generates an overflow notification every N events, where N is
    /// given by sample_period.  A sampling event has sample_period > 0.  When an overflow occurs,
    /// requested data is recorded in the mmap buffer.  The sample_type field controls what data is
    /// recorded on each overflow.
    Period(u64),
    /// Can be used if you wish to use frequency rather than period.  In this case, you set the
    /// freq flag.  The kernel will adjust the sampling period to try and achieve the desired
    /// rate.  The rate of adjustment is a timer tick.
    Frequency(u64),
}

impl SamplingRate {
    fn apply(&self, attr: &mut perf_event_attr) {
        use raw::perf_event_attr__bindgen_ty_1;
        let sample_config = match *self {
            SamplingRate::Period(p) => perf_event_attr__bindgen_ty_1 { sample_period: p },
            SamplingRate::Frequency(f) => {
                attr.set_freq(1);
                perf_event_attr__bindgen_ty_1 { sample_freq: f }
            }
        };

        attr.__bindgen_anon_1 = sample_config;
    }
}

// wakeup_events counts only PERF_RECORD_SAMPLE record types.  To
// receive overflow notification for all PERF_RECORD types choose
// watermark and set wakeup_watermark to 1.

// Prior to Linux 3.0, setting wakeup_events to 0 resulted in no
// overflow notifications; more recent kernels treat 0 the same
// as 1.

//           This union sets how many samples (wakeup_events) or bytes
//           (wakeup_watermark) happen before an overflow notification hap‐
//           pens.  Which one is used is selected by the watermark bit
//           flag.
#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord, Serialize)]
pub enum WakeupConfig {
    NumSamples(u32),
    WatermarkBytes(u32),
}

impl WakeupConfig {
    fn apply(&self, attr: &mut perf_event_attr) {
        use raw::perf_event_attr__bindgen_ty_2;
        let wakeup = match *self {
            WakeupConfig::NumSamples(n) => perf_event_attr__bindgen_ty_2 { wakeup_events: n },
            WakeupConfig::WatermarkBytes(b) => {
                attr.set_watermark(1);
                perf_event_attr__bindgen_ty_2 {
                    wakeup_watermark: b,
                }
            }
        };
        attr.__bindgen_anon_2 = wakeup;
    }
}

pub mod sampled {
    use super::*;
    macro_rules! sampled_spec {
        ($struct_name:ident, $flag:ident) => {
            pub struct $struct_name;

            impl SampleThingy for $struct_name {
                const FLAGS: SamplingType = SamplingType::$flag;
            }
        };
    }

    sampled_spec!(InstructionPointer, IP);
    sampled_spec!(Address, ADDR);
    sampled_spec!(Read, READ);
    sampled_spec!(Callchain, CALLCHAIN);
    sampled_spec!(Period, PERIOD);
    sampled_spec!(Raw, RAW);
    sampled_spec!(BranchStack, BRANCH_STACK);
    sampled_spec!(RegistersUser, REGS_USER);
    sampled_spec!(StackUser, STACK_USER);
    sampled_spec!(Weight, WEIGHT);
    sampled_spec!(DataSource, DATA_SRC);
    sampled_spec!(Transaction, TRANSACTION);
    sampled_spec!(RegistersIntr, REGS_INTR);

}

pub trait SampleThingy {
    const FLAGS: SamplingType;
}

use raw::perf_event_sample_format::*;
bitflags! {
    /// The various bits in this field specify which values to include in the sample.  They will be
    /// recorded in a ring-buffer, which is available to user space using mmap(2). The order in
    /// which the values are saved in the sample are documented in the MMAP Layout subsection; it is
    /// not the enum perf_event_sample_format order.
    #[derive(Serialize)]
    pub struct SamplingType: u32 {
        /// Records instruction pointer.
        const IP = PERF_SAMPLE_IP;

        /// Records the process and thread IDs.
        const TID = PERF_SAMPLE_TID;

        /// Records a timestamp.
        const TIME = PERF_SAMPLE_TIME;

        /// Records an address, if applicable.
        const ADDR = PERF_SAMPLE_ADDR;

        /// Record counter values for all events in a group, not just the group leader.
        const READ = PERF_SAMPLE_READ;

        /// Records the callchain (stack backtrace).
        const CALLCHAIN = PERF_SAMPLE_CALLCHAIN;

        /// Records a unique ID for the opened event's group leader.
        const ID = PERF_SAMPLE_ID;

        /// Records CPU number.
        const CPU = PERF_SAMPLE_CPU;

        /// Records the current sampling period.
        const PERIOD = PERF_SAMPLE_PERIOD;

        /// Records a unique ID for the opened event. Unlike PERF_SAMPLE_ID the actual ID is
        /// returned, not the group leader. This ID is the same as the one returned by
        /// PERF_FORMAT_ID.
        const STREAM_ID = PERF_SAMPLE_STREAM_ID;

        /// Records additional data, if applicable.  Usually returned by tracepoint events.
        const RAW = PERF_SAMPLE_RAW;

        /// This provides a record of recent branches, as provided by CPU branch sampling hardware
        /// (such as Intel Last Branch Record). Not all hardware supports this feature. (since Linux
        /// 3.4)
        ///
        /// See the branch_sample_type field for how to filter which branches are reported.
        const BRANCH_STACK = PERF_SAMPLE_BRANCH_STACK;

        /// Records the current user-level CPU register state (the values in the process before the
        /// kernel was called). (since Linux 3.7)
        const REGS_USER = PERF_SAMPLE_REGS_USER;

        /// Records the user level stack, allowing stack unwinding. (since Linux 3.7)
        const STACK_USER = PERF_SAMPLE_STACK_USER;

        /// Records a hardware provided weight value that expresses how costly the sampled event
        /// was. This allows the hardware to highlight expensive events in a profile.
        /// (since Linux 3.10)
        const WEIGHT = PERF_SAMPLE_WEIGHT;

        /// Records the data source: where in the memory hierarchy the data associated with the
        /// sampled instruction came from. This is available only if the underlying hardware
        /// supports this feature. (since Linux 3.10)
        const DATA_SRC = PERF_SAMPLE_DATA_SRC;

        /// Places the SAMPLE_ID value in a fixed position in the record, either at the beginning
        /// (for sample events) or at the end (if a non-sample event). (since Linux 3.12)
        ///
        /// This was necessary because a sample stream may have records from various different event
        /// sources with different sample_type settings. Parsing the event stream properly was not
        /// possible because the format of the record was needed to find SAMPLE_ID, but the format
        /// could not be found without knowing what event the sam‐ ple belonged to (causing a
        /// circular dependency).
        ///
        /// The PERF_SAMPLE_IDENTIFIER setting makes the event stream always parsable by putting
        /// SAMPLE_ID in a fixed location, even though it means having duplicate SAMPLE_ID values in
        /// records.
        const IDENTIFIER = PERF_SAMPLE_IDENTIFIER;

        /// Records reasons for transactional memory abort events (for example, from Intel TSX
        /// transactional memory support).
        ///
        /// The precise_ip setting must be greater than 0 and a transactional memory abort event
        /// must be measured or no values will be recorded. Also note that some perf_event
        /// measurements, such as sampled cycle counting, may cause extraneous aborts (by causing an
        /// interrupt during a transaction).
        const TRANSACTION = PERF_SAMPLE_TRANSACTION;

        /// Records a subset of the current CPU register state as specified by sample_regs_intr.
        /// Unlike PERF_SAMPLE_REGS_USER the register values will return kernel register state if
        /// the overflow happened while kernel code is running. If the CPU supports hardware
        /// sampling of register state (i.e., PEBS on Intel x86) and precise_ip is set higher than
        /// zero then the register values returned are those captured by hardware at the time of the
        /// sampled instruction's retirement. (since Linux 3.19)
        const REGS_INTR = PERF_SAMPLE_REGS_INTR;
    }
}

// macro_rules! sample_id_spec {
//     () => {
//         SampleId<(), (), (), (), (), ()>
//     };
//     (TID, SampleId<(),  $time:ty, $id:ty, $sid:ty, $cpu:ty, $idn:ty>) => {
//           SampleId<Tid, $time,    $id,    $sid,    $cpu,    $idn>
//     };
//     (TIME, SampleId<$tid:ty, (),   $id:ty, $sid:ty, $cpu:ty, $idn:ty>) => {
//            SampleId<$tid,       Time, $id,       $sid,       $cpu,       $idn>
//     };
//     // (ID, SampleId<$tid:ty, $time:ty, (), $sid:ty, $cpu:ty, $idn:ty>) => {
//     //      SampleId<$tid,       $time,       Id, $sid,       $cpu,       $idn>
//     // };
//     // (STREAM_ID, SampleId<$tid:ident, $time:ident, $id:ident, (),       $cpu:ident, $idn:ident>) => {
//     //             SampleId<$tid,       $time,       $id,       StreamId, $cpu,       $idn>
//     // };
//     // (CPU, SampleId<$tid:ident, $time:ident, $id:ident, $sid:ident, (),  $idn:ident>) => {
//     //       SampleId<$tid,       $time,       $id,       $sid,       Cpu, $idn>
//     // };
//     // (IDENT, SampleId<$tid:ident, $time:ident, $id:ident, $sid:ident, $cpu:ident, ()>) => {
//     //         SampleId<$tid,       $time,       $id,       $sid,       $cpu,       Identifier>
//     // };
//     ($flag:expr, $rest:tt) => {
//         sample_id_spec!($flag, sample_id_spec!($rest))
//     };
//     ($base:expr) => {
//         sample_id_spec!(TIME, SampleId<(), (), (), (), (), ()>)
//     };
// }

pub type AllSampleIds = SampleId<Tid, Time, Id, StreamId, Cpu, Identifier>;
pub type NoSampleIds = SampleId<(), (), (), (), (), ()>;

//   struct sample_id {
//       { u32 pid, tid; }   /* if PERF_SAMPLE_TID set */
//       { u64 time;     }   /* if PERF_SAMPLE_TIME set */
//       { u64 id;       }   /* if PERF_SAMPLE_ID set */
//       { u64 stream_id;}   /* if PERF_SAMPLE_STREAM_ID set  */
//       { u32 cpu, res; }   /* if PERF_SAMPLE_CPU set */
//       { u64 id;       }   /* if PERF_SAMPLE_IDENTIFIER set */
//   };
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SampleId<TID, TIME, ID, SID, CPU, IDN>
where
    TID: ThreadIdSpec,
    TIME: TimeSpec,
    ID: IdSpec,
    SID: StreamIdSpec,
    CPU: CpuSpec,
    IDN: IdSpec,
{
    pub tid: TID,
    pub time: TIME,
    pub id: ID,
    pub stream_id: SID,
    pub cpu: CPU,
    pub ident: IDN,
}

macro_rules! sample_id_field {
        {
            $struct_name:ident : $trait_name:ident {
                $( $field_name:ident : $field_type:ty, )*
            },
            $with_name:ident,
            $flag:ident
        } => {
            #[repr(C)]
            #[derive(Clone, Copy, Default)]
            pub struct $struct_name {
                $(
                    $field_name: $field_type,
                )*
            }

            pub trait $trait_name: Copy + Default {}
            impl $trait_name for () {}
            impl $trait_name for $struct_name {}

            impl SampleThingy for $struct_name {
                const FLAGS: SamplingType = SamplingType::$flag;
            }
        };
    }

sample_id_field! { Tid: ThreadIdSpec { pid: u32, tid: u32, }, WithTid, TID }
sample_id_field! { Time: TimeSpec { time: u64, }, WithTime, TIME }
sample_id_field! { Id: IdSpec { id: u64, }, WithId, ID }
sample_id_field! { StreamId: StreamIdSpec { stream_id: u64, }, WithStreamId, STREAM_ID }
sample_id_field! { Cpu: CpuSpec { cpu: u32, res: u32, }, WithCpu, CPU }
sample_id_field! { Identifier: IdentifierSpec { id: u64, }, WithIdentifier, IDENTIFIER }

/*
           branch_sample_type (since Linux 3.4)
              If PERF_SAMPLE_BRANCH_STACK is enabled, then this specifies
              what branches to include in the branch record.

              The first part of the value is the privilege level, which is a
              combination of one of the values listed below.  If the user
              does not set privilege level explicitly, the kernel will use
              the event's privilege level.  Event and branch privilege lev‐
              els do not have to match.

              PERF_SAMPLE_BRANCH_USER
                     Branch target is in user space.

              PERF_SAMPLE_BRANCH_KERNEL
                     Branch target is in kernel space.

              PERF_SAMPLE_BRANCH_HV
                     Branch target is in hypervisor.

              PERF_SAMPLE_BRANCH_PLM_ALL
                     A convenience value that is the three preceding values
                     ORed together.

              In addition to the privilege value, at least one or more of
              the following bits must be set.

              PERF_SAMPLE_BRANCH_ANY
                     Any branch type.

              PERF_SAMPLE_BRANCH_ANY_CALL
                     Any call branch (includes direct calls, indirect calls,
                     and far jumps).

              PERF_SAMPLE_BRANCH_IND_CALL
                     Indirect calls.

              PERF_SAMPLE_BRANCH_CALL (since Linux 4.4)
                     Direct calls.

              PERF_SAMPLE_BRANCH_ANY_RETURN
                     Any return branch.

              PERF_SAMPLE_BRANCH_IND_JUMP (since Linux 4.2)
                     Indirect jumps.

              PERF_SAMPLE_BRANCH_COND (since Linux 3.16)
                     Conditional branches.

              PERF_SAMPLE_BRANCH_ABORT_TX (since Linux 3.11)
                     Transactional memory aborts.

              PERF_SAMPLE_BRANCH_IN_TX (since Linux 3.11)
                     Branch in transactional memory transaction.

              PERF_SAMPLE_BRANCH_NO_TX (since Linux 3.11)
                     Branch not in transactional memory transaction.
                     PERF_SAMPLE_BRANCH_CALL_STACK (since Linux 4.1) Branch
                     is part of a hardware-generated call stack.  This
                     requires hardware support, currently only found on
                     Intel x86 Haswell or newer.

*/
