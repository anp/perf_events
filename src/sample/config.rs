use fd::PerfEventAttrThingy;
use raw::perf_event_attr;

#[derive(Clone, Debug, Eq, PartialEq, PartialOrd, Ord, Serialize)]
pub struct SamplingConfig {
    rate: SamplingRate,
    requests: Vec<SampleRequest>,
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

impl PerfEventAttrThingy for SamplingConfig {
    fn apply(&self, attr: &mut perf_event_attr) {
        use count::SwEvent;
        use raw::perf_type_id;

        attr.type_ = perf_type_id::PERF_TYPE_SOFTWARE;
        attr.config = SwEvent::DummyForSampled as u64;

        self.rate.apply(attr);
        self.wakeup.apply(attr);
        for request in &self.requests {
            request.apply(attr);
        }
    }
}

use std::fmt::{Display, Formatter, Result as FmtResult};

impl Display for SamplingConfig {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        f.write_str("Sampled: ")
            .and_then(|()| f.write_fmt(format_args!("{:?}", self)))
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

/// Specifies which values to include in the sample. They will be recorded in a ring-buffer, which
/// is available to user space using mmap(2). The order in which the values are saved in the sample
/// are documented in the MMAP Layout subsection; it is not the enum perf_event_sample_format order.
#[derive(Clone, Debug, Eq, PartialEq, PartialOrd, Ord, Serialize)]
pub enum SampleRequest {
    /// Records instruction pointer.
    InstructionPointer,

    /// Records an address, if applicable.
    Address,

    /// Record counter values for all events in a group, not just the group leader.
    Read,

    /// Records the callchain (stack backtrace).
    Callchain,

    /// Records the current sampling period.
    Period,

    /// Records additional data, if applicable.  Usually returned by tracepoint events.
    Raw,

    /// Records the current user-level CPU register state (the values in the process before the
    /// kernel was called). (since Linux 3.7)
    RegistersUser,

    /// Records the user level stack, allowing stack unwinding. (since Linux 3.7)
    StackUser,

    /// Records a hardware provided weight value that expresses how costly the sampled event
    /// was. This allows the hardware to highlight expensive events in a profile.
    /// (since Linux 3.10)
    Weight,

    /// Records the data source: where in the memory hierarchy the data associated with the
    /// sampled instruction came from. This is available only if the underlying hardware
    /// supports this feature. (since Linux 3.10)
    DataSource,

    /// Records reasons for transactional memory abort events (for example, from Intel TSX
    /// transactional memory support).
    ///
    /// The precise_ip setting must be greater than 0 and a transactional memory abort event
    /// must be measured or no values will be recorded. Also note that some perf_event
    /// measurements, such as sampled cycle counting, may cause extraneous aborts (by causing an
    /// interrupt during a transaction).
    Transaction,

    /// Records a subset of the current CPU register state as specified by sample_regs_intr.
    /// Unlike PERF_SAMPLE_REGS_USER the register values will return kernel register state if
    /// the overflow happened while kernel code is running. If the CPU supports hardware
    /// sampling of register state (i.e., PEBS on Intel x86) and precise_ip is set higher than
    /// zero then the register values returned are those captured by hardware at the time of the
    /// sampled instruction's retirement. (since Linux 3.19)
    RegistersIntr,

    /// This provides a record of recent branches, as provided by CPU branch sampling hardware
    /// (such as Intel Last Branch Record). Not all hardware supports this feature. (since Linux
    /// 3.4)
    ///
    /// See the branch_sample_type field for how to filter which branches are reported.
    BranchStack(BranchSamplePriv, BranchSampleType),
}

impl SampleRequest {
    fn apply(&self, attr: &mut perf_event_attr) {
        use self::SampleRequest::*;
        use raw::perf_event_sample_format::*;
        attr.sample_type = match *self {
            InstructionPointer => PERF_SAMPLE_IP,
            Address => PERF_SAMPLE_ADDR,
            Read => PERF_SAMPLE_READ,
            Callchain => PERF_SAMPLE_CALLCHAIN,
            Period => PERF_SAMPLE_PERIOD,
            Raw => PERF_SAMPLE_RAW,
            RegistersUser => PERF_SAMPLE_REGS_USER,
            StackUser => PERF_SAMPLE_STACK_USER,
            Weight => PERF_SAMPLE_WEIGHT,
            DataSource => PERF_SAMPLE_DATA_SRC,
            Transaction => PERF_SAMPLE_TRANSACTION,
            RegistersIntr => PERF_SAMPLE_REGS_INTR,
            BranchStack(_, _) => {
                // TODO set up the stuff
                PERF_SAMPLE_BRANCH_STACK
            }
        } as u64;
    }
}

// NOTE(anp): the next two types have their bits OR'd to make up the sample_branch_type value in
// perf_event_attr;

use raw::perf_branch_sample_type::*;
bitflags! {
    /// If SamplingType::BRANCH_STACK is enabled, then this specifies what branches to include in
    /// the branch record. els do not have to match. (since Linux 3.4)
    #[derive(Serialize)]
    pub struct BranchSamplePriv: u32 {
        /// Branch target is in user space.
        const USER = PERF_SAMPLE_BRANCH_USER;

        /// Branch target is in kernel space.
        const KERNEL = PERF_SAMPLE_BRANCH_KERNEL;

        /// Branch target is in hypervisor.
        const HV = PERF_SAMPLE_BRANCH_HV;

        /// A convenience value that is the three preceding values ORed together.
        const ALL = Self::USER.bits | Self::KERNEL.bits | Self::HV.bits;
    }
}

bitflags! {
    /// In addition to the privilege value, at least one or more of the following bits must be set.
    #[derive(Serialize)]
    pub struct BranchSampleType: u32 {
        /// Any branch type.
        const ANY = PERF_SAMPLE_BRANCH_ANY;

        /// Any call branch (includes direct calls, indirect calls, and far jumps).
        const ANY_CALL = PERF_SAMPLE_BRANCH_ANY_CALL;

        /// Indirect calls.
        const IND_CALL = PERF_SAMPLE_BRANCH_IND_CALL;

        /// Direct calls. (since Linux 4.4)
        const CALL = PERF_SAMPLE_BRANCH_CALL;

        /// Any return branch.
        const ANY_RETURN = PERF_SAMPLE_BRANCH_ANY_RETURN;

        /// Indirect jumps. (since Linux 4.2)
        const IND_JUMP = PERF_SAMPLE_BRANCH_IND_JUMP;

        /// Conditional branches. (since Linux 3.16)
        const COND = PERF_SAMPLE_BRANCH_COND;

        /// Transactional memory aborts. (since Linux 3.11)
        const ABORT_TX = PERF_SAMPLE_BRANCH_ABORT_TX;

        /// Branch in transactional memory transaction. (since Linux 3.11)
        const IN_TX = PERF_SAMPLE_BRANCH_IN_TX;

        /// Branch not in transactional memory transaction. (since Linux 4.1)
        const NO_TX = PERF_SAMPLE_BRANCH_NO_TX;

        /// Branch is part of a hardware-generated call stack. This requires hardware support,
        /// currently only found on Intel x86 Haswell or newer. (since Linux 3.11)
        const CALL_STACK = PERF_SAMPLE_BRANCH_CALL_STACK;
    }
}

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
        };
    }

/// Records the process and thread IDs.
sample_id_field! { Tid: ThreadIdSpec { pid: u32, tid: u32, }, WithTid, PERF_SAMPLE_TID }

/// Records a timestamp.
sample_id_field! { Time: TimeSpec { time: u64, }, WithTime, PERF_SAMPLE_TIME }

/// Records a unique ID for the opened event's group leader.
sample_id_field! { Id: IdSpec { id: u64, }, WithId, PERF_SAMPLE_ID }

/// Records a unique ID for the opened event. Unlike PERF_SAMPLE_ID the actual ID is
/// returned, not the group leader. This ID is the same as the one returned by
/// PERF_FORMAT_ID.
sample_id_field! { StreamId: StreamIdSpec { stream_id: u64, }, WithStreamId, PERF_SAMPLE_STREAM_ID }

/// Records CPU number.
sample_id_field! { Cpu: CpuSpec { cpu: u32, res: u32, }, WithCpu, PERF_SAMPLE_CPU }

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
sample_id_field! { Identifier: IdentifierSpec { id: u64, }, WithIdentifier, PERF_SAMPLE_IDENTIFIER }
