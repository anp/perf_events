extern crate errno;
extern crate failure;
#[macro_use]
extern crate failure_derive;
extern crate libc;

// TODO logging

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::os::unix::io::{FromRawFd, RawFd};

use errno::{errno, Errno};
use libc::{c_int, pid_t, syscall, SYS_perf_event_open};

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

    pub fn start(&mut self) -> Vec<Result<(), Errno>> {
        // TODO ioctl enable -- enable_on_exec?
        unimplemented!();
    }

    pub fn read(&mut self) -> Vec<(Event, u64)> {
        unimplemented!();
    }

    pub fn start_all_available() -> Result<Self, PerfEventsError> {
        let res = Counts::new(PidConfig::Current, CpuConfig::All)
            .all_available()
            .create();

        if let (_, Err(ref failures)) = res {
            for (event, error) in failures {
                // TODO log this
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

pub struct CountsBuilder {
    pid: PidConfig,
    cpu: CpuConfig,
    to_count: BTreeSet<Event>,
}

impl CountsBuilder {
    pub fn all_available(self) -> Self {
        // TODO
        unimplemented!();
    }

    pub fn event(mut self, event: Event) -> Self {
        self.to_count.insert(event);
        self
    }

    pub fn create(self) -> (Result<Counts, ()>, Result<(), BTreeMap<Event, OpenError>>) {
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
            PidConfig::Current => -1,
            PidConfig::Other(p) => p,
        }
    }
}

#[derive(Clone, Copy, Debug)]
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

#[derive(Debug)]
struct EventCounter {
    event: Event,
    file: File,
}

impl EventCounter {
    fn new(event: Event, pid: PidConfig, cpu: CpuConfig) -> Result<Self, OpenError> {
        let file = unsafe { File::from_raw_fd(event.create_fd(pid, cpu)?) };
        Ok(Self { event, file })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub enum Event {
    Hardware(HwEvent),
    Software(SwEvent),
    HardwareCache(CacheId, CacheOpId, CacheOpResultId),
}

#[derive(Debug, Fail)]
pub enum OpenError {
    #[fail(display = "Returned if the perf_event_attr size value is too small
              (smaller than PERF_ATTR_SIZE_VER0), too big (larger than the
              page size), or larger than the kernel supports and the extra
              bytes are not zero.  When E2BIG is returned, the
              perf_event_attr size field is overwritten by the kernel to be
              the size of the structure it was expecting.")]
    AttrWrongSize,
    #[fail(display = "Returned when the requested event requires CAP_SYS_ADMIN
              permissions (or a more permissive perf_event paranoid
              setting).  Some common cases where an unprivileged process may
              encounter this error: attaching to a process owned by a
              different user; monitoring all processes on a given CPU (i.e.,
              specifying the pid argument as -1); and not setting
              exclude_kernel when the paranoid setting requires it.")]
    CapSysAdminRequired,
    #[fail(display = "Returned if the group_fd file descriptor is not valid, or, if
              PERF_FLAG_PID_CGROUP is set, the cgroup file descriptor in pid
              is not valid.")]
    InvalidFdOrPid,
    #[fail(display = "Returned if another event already has exclusive access to the
              PMU.")]
    PmuBusy,
    #[fail(display = "Returned if the attr pointer points at an invalid memory
              address.")]
    AttrInvalidPointer,
    #[fail(display = "Returned if the specified event is invalid.  There are many
              possible reasons for this.  A not-exhaustive list: sample_freq
              is higher than the maximum setting; the cpu to monitor does
              not exist; read_format is out of range; sample_type is out of
              range; the flags value is out of range; exclusive or pinned
              set and the event is not a group leader; the event config
              values are out of range or set reserved bits; the generic
              event selected is not supported; or there is not enough room
              to add the selected event.")]
    InvalidEvent,
    #[fail(display = "Each opened event uses one file descriptor.  If a large number
              of events are opened, the per-process limit on the number of
              open file descriptors will be reached, and no more events can
              be created.")]
    TooManyOpenFiles,
    #[fail(display = "Returned when the event involves a feature not supported by
              the current CPU.")]
    CpuFeatureUnsupported,
    #[fail(display = "Returned if the type setting is not valid.  This error is also
              returned for some unsupported generic events.")]
    InvalidEventType,
    #[fail(display = "Prior to Linux 3.3, if there was not enough room for the
              event, ENOSPC was returned.  In Linux 3.3, this was changed to
              EINVAL.  ENOSPC is still returned if you try to add more
              breakpoint events than supported by the hardware.")]
    TooManyBreakpoints,
    #[fail(display = "Returned if PERF_SAMPLE_STACK_USER is set in sample_type and
              it is not supported by hardware.")]
    UserStackSampleUnsupported,
    #[fail(display = "Returned if an event requiring a specific hardware feature is
              requested but there is no hardware support.  This includes
              requesting low-skid events if not supported, branch tracing if
              it is not available, sampling if no PMU interrupt is
              available, and branch stacks for software events.")]
    HardwareFeatureUnsupported,
    #[fail(display = "(since Linux 4.8)
              Returned if PERF_SAMPLE_CALLCHAIN is requested and
              sample_max_stack is larger than the maximum specified in
              /proc/sys/kernel/perf_event_max_stack.")]
    SampleMaxStackTooLarge,
    #[fail(display = "Returned on many (but not all) architectures when an
              unsupported exclude_hv, exclude_idle, exclude_user, or
              exclude_kernel setting is specified.

              It can also happen, as with EACCES, when the requested event
              requires CAP_SYS_ADMIN permissions (or a more permissive
              perf_event paranoid setting).  This includes setting a
              breakpoint on a kernel address, and (since Linux 3.13) setting
              a kernel function-trace tracepoint.")]
    CapSysAdminRequiredOrExcludeUnsupported,
    #[fail(display = "Returned if attempting to attach to a process that does not
              exist.")]
    ProcessDoesNotExist,
    #[fail(display = "The kernel returned an unexpected error code: {}", errno)]
    Unknown { errno: Errno },
}

impl From<Errno> for OpenError {
    fn from(errno: Errno) -> OpenError {
        match errno.0 {
            libc::E2BIG => OpenError::AttrWrongSize,
            libc::EACCES => OpenError::CapSysAdminRequired,
            libc::EBADF => OpenError::InvalidFdOrPid,
            libc::EBUSY => OpenError::PmuBusy,
            libc::EFAULT => OpenError::AttrInvalidPointer,
            libc::EINVAL => OpenError::InvalidEvent,
            libc::EMFILE => OpenError::TooManyOpenFiles,
            libc::ENODEV => OpenError::CpuFeatureUnsupported,
            libc::ENOENT => OpenError::InvalidEventType,
            libc::ENOSPC => OpenError::TooManyBreakpoints,
            libc::ENOSYS => OpenError::UserStackSampleUnsupported,
            libc::EOPNOTSUPP => OpenError::HardwareFeatureUnsupported,
            libc::EOVERFLOW => OpenError::SampleMaxStackTooLarge,
            libc::EPERM => OpenError::CapSysAdminRequiredOrExcludeUnsupported,
            libc::ESRCH => OpenError::ProcessDoesNotExist,
            _ => OpenError::Unknown { errno },
        }
    }
}

impl Event {
    fn create_fd(&self, pid: PidConfig, cpu: CpuConfig) -> Result<RawFd, OpenError> {
        unsafe {
            match syscall(
                SYS_perf_event_open,
                &self.as_raw(true),
                pid.raw(),
                cpu.raw(),
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
                -1 => Err(errno().into()),
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

#[derive(Debug, Fail)]
pub enum PerfEventsError {
    #[fail(display = "Failed to open a perf_events file descriptor: {}", inner)]
    FdOpenError { inner: OpenError },
    #[fail(display = "Failed to start collecting metrics: {}", inner)]
    StartError { inner: String },
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_one_shot() {
        let mut counts = Counts::start_all_available().unwrap();
        let before = counts.read();

        println!("first:\n{:#?}", before);

        for i in 0..10000 {
            // noop
        }

        let after = counts.read();
        println!("second:\n{:#?}", after);
    }
}
