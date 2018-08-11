use std::fs::File;
use std::io;
use std::io::Read;
use std::ops::{Deref, DerefMut};
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};

use libc::{syscall, SYS_perf_event_open};
use nix::errno::Errno;

use super::{CpuConfig, PidConfig};
use events::Event;

#[derive(Debug)]
pub struct PerfEventFile(pub(crate) File, pub(crate) Event);

impl PerfEventFile {
    pub fn new(event: Event, pid: PidConfig, cpu: CpuConfig) -> Result<Self> {
        // NOTE(unsafe) it'd be a kernel bug if this caused unsafety, i think
        unsafe {
            match syscall(
                SYS_perf_event_open,
                &event.as_raw(true),
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
                -1 => return Err(Error::from(OpenError::from(Errno::last()))),
                // NOTE(unsafe) if the kernel doesn't give -1, guarantees the fd is valid
                fd => Ok(PerfEventFile(File::from_raw_fd(fd as i32), event)),
            }
        }
    }
}

#[derive(Debug, Fail)]
pub enum OpenError {
    #[fail(
        display = "Returned if the perf_event_attr size value is too small
              (smaller than PERF_ATTR_SIZE_VER0), too big (larger than the
              page size), or larger than the kernel supports and the extra
              bytes are not zero.  When E2BIG is returned, the
              perf_event_attr size field is overwritten by the kernel to be
              the size of the structure it was expecting."
    )]
    AttrWrongSize,
    #[fail(
        display = "Returned when the requested event requires CAP_SYS_ADMIN
              permissions (or a more permissive perf_event paranoid
              setting).  Some common cases where an unprivileged process may
              encounter this error: attaching to a process owned by a
              different user; monitoring all processes on a given CPU (i.e.,
              specifying the pid argument as -1); and not setting
              exclude_kernel when the paranoid setting requires it."
    )]
    CapSysAdminRequired,
    #[fail(
        display = "Returned if the group_fd file descriptor is not valid, or, if
              PERF_FLAG_PID_CGROUP is set, the cgroup file descriptor in pid
              is not valid."
    )]
    InvalidFdOrPid,
    #[fail(
        display = "Returned if another event already has exclusive access to the
              PMU."
    )]
    PmuBusy,
    #[fail(
        display = "Returned if the attr pointer points at an invalid memory
              address."
    )]
    AttrInvalidPointer,
    #[fail(
        display = "Returned if the specified event is invalid.  There are many
              possible reasons for this.  A not-exhaustive list: sample_freq
              is higher than the maximum setting; the cpu to monitor does
              not exist; read_format is out of range; sample_type is out of
              range; the flags value is out of range; exclusive or pinned
              set and the event is not a group leader; the event config
              values are out of range or set reserved bits; the generic
              event selected is not supported; or there is not enough room
              to add the selected event."
    )]
    InvalidEvent,
    #[fail(
        display = "Each opened event uses one file descriptor.  If a large number
              of events are opened, the per-process limit on the number of
              open file descriptors will be reached, and no more events can
              be created."
    )]
    TooManyOpenFiles,
    #[fail(
        display = "Returned when the event involves a feature not supported by
              the current CPU."
    )]
    CpuFeatureUnsupported,
    #[fail(
        display = "Returned if the type setting is not valid.  This error is also
              returned for some unsupported generic events."
    )]
    InvalidEventType,
    #[fail(
        display = "Prior to Linux 3.3, if there was not enough room for the
              event, ENOSPC was returned.  In Linux 3.3, this was changed to
              EINVAL.  ENOSPC is still returned if you try to add more
              breakpoint events than supported by the hardware."
    )]
    TooManyBreakpoints,
    #[fail(
        display = "Returned if PERF_SAMPLE_STACK_USER is set in sample_type and
              it is not supported by hardware."
    )]
    UserStackSampleUnsupported,
    #[fail(
        display = "Returned if an event requiring a specific hardware feature is
              requested but there is no hardware support.  This includes
              requesting low-skid events if not supported, branch tracing if
              it is not available, sampling if no PMU interrupt is
              available, and branch stacks for software events."
    )]
    HardwareFeatureUnsupported,
    #[fail(
        display = "(since Linux 4.8)
              Returned if PERF_SAMPLE_CALLCHAIN is requested and
              sample_max_stack is larger than the maximum specified in
              /proc/sys/kernel/perf_event_max_stack."
    )]
    SampleMaxStackTooLarge,
    #[fail(
        display = "Returned on many (but not all) architectures when an
              unsupported exclude_hv, exclude_idle, exclude_user, or
              exclude_kernel setting is specified.

              It can also happen, as with EACCES, when the requested event
              requires CAP_SYS_ADMIN permissions (or a more permissive
              perf_event paranoid setting).  This includes setting a
              breakpoint on a kernel address, and (since Linux 3.13) setting
              a kernel function-trace tracepoint."
    )]
    CapSysAdminRequiredOrExcludeUnsupported,
    #[fail(
        display = "Returned if attempting to attach to a process that does not
              exist."
    )]
    ProcessDoesNotExist,
    #[fail(display = "The kernel returned an unexpected error code: {}", errno)]
    Unknown { errno: Errno },
}

impl From<Errno> for OpenError {
    fn from(errno: Errno) -> OpenError {
        match errno {
            Errno::E2BIG => OpenError::AttrWrongSize,
            Errno::EACCES => OpenError::CapSysAdminRequired,
            Errno::EBADF => OpenError::InvalidFdOrPid,
            Errno::EBUSY => OpenError::PmuBusy,
            Errno::EFAULT => OpenError::AttrInvalidPointer,
            Errno::EINVAL => OpenError::InvalidEvent,
            Errno::EMFILE => OpenError::TooManyOpenFiles,
            Errno::ENODEV => OpenError::CpuFeatureUnsupported,
            Errno::ENOENT => OpenError::InvalidEventType,
            Errno::ENOSPC => OpenError::TooManyBreakpoints,
            Errno::ENOSYS => OpenError::UserStackSampleUnsupported,
            Errno::EOPNOTSUPP => OpenError::HardwareFeatureUnsupported,
            Errno::EOVERFLOW => OpenError::SampleMaxStackTooLarge,
            Errno::EPERM => OpenError::CapSysAdminRequiredOrExcludeUnsupported,
            Errno::ESRCH => OpenError::ProcessDoesNotExist,
            _ => OpenError::Unknown { errno },
        }
    }
}

const PERF_EVENT_IOC_MAGIC: u8 = b'$';
const PERF_EVENT_IOC_ENABLE_MODE: u8 = 0;

ioctl!(
    none
    perf_event_ioc_enable
    with
    PERF_EVENT_IOC_MAGIC,
    PERF_EVENT_IOC_ENABLE_MODE
);

impl Read for PerfEventFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

impl AsRawFd for PerfEventFile {
    fn as_raw_fd(&self) -> RawFd {
        self.0.as_raw_fd()
    }
}

impl Deref for PerfEventFile {
    type Target = File;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for PerfEventFile {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

use std::io::Result as IoResult;

use mio::event::Evented;
use mio::unix::EventedFd;
use mio::{Poll, PollOpt, Ready, Token};
use mmap::{MapOption, MemoryMap};
use page_size::get as page_size;

use error::*;
use raw::*;

pub enum SampledRecord {}

/// The mmap values start with a header.
struct EventHeader {
    event_type: SampledEventType,
    misc: Misc,
    size: u16,
}

impl<'a> From<&'a perf_event_header> for EventHeader {
    fn from(raw: &perf_event_header) -> Self {
        Self {
            size: raw.size,
            event_type: SampledEventType::from(raw.type_),
            misc: Misc::from(raw.misc),
        }
    }
}

/// The misc field contains additional information about the sample.
///
/// Note: we do not support accessing the PERF_RECORD_MISC_PROC_MAP_PARSE_TIMEOUT
/// value. From the syscall's doc page:
///     This bit is not set by the kernel.  It is reserved for the user-space perf
///     utility to indicate that /proc/i[pid]/maps parsing was taking too long and was
///     stopped, and thus the mmap records may be truncated.
struct Misc {
    cpu_mode: CpuMode,
    /// Since the following three statuses are generated by different
    /// record types, they alias to the same bit, which is represented here as
    /// a bool:
    ///
    /// `PERF_RECORD_MISC_MMAP_DATA` (since Linux 3.10)
    ///        This is set when the mapping is not executable; other‐
    ///        wise the mapping is executable.
    ///
    /// `PERF_RECORD_MISC_COMM_EXEC` (since Linux 3.16)
    ///        This is set for a PERF_RECORD_COMM record on kernels
    ///        more recent than Linux 3.16 if a process name change
    ///        was caused by an exec(2) system call.
    ///
    /// `PERF_RECORD_MISC_SWITCH_OUT` (since Linux 4.3)
    ///        When a PERF_RECORD_SWITCH or
    ///        PERF_RECORD_SWITCH_CPU_WIDE record is generated, this
    ///        bit indicates that the context switch is away from the
    ///        current process (instead of into the current process).
    multipurpose_lol: bool,
    /// This indicates that the content of PERF_SAMPLE_IP points to the actual instruction that
    /// triggered the event.  See also perf_event_attr.precise_ip. (PERF_RECORD_MISC_EXACT_IP)
    exact_ip: bool,
    /// This indicates there is extended data available (currently not used).
    /// (PERF_RECORD_MISC_EXT_RESERVED, since Linux 2.6.35)
    reserved: bool,
}

impl From<u16> for Misc {
    fn from(n: u16) -> Self {
        Self {
            cpu_mode: CpuMode::from(n),
            multipurpose_lol: (n as u32 | PERF_RECORD_MISC_MMAP_DATA) != 0,
            exact_ip: (n as u32 | PERF_RECORD_MISC_EXACT_IP) != 0,
            reserved: (n as u32 | PERF_RECORD_MISC_EXT_RESERVED) != 0,
        }
    }
}

/// The CPU mode can be determined from this value.
enum CpuMode {
    /// Unknown CPU mode. (PERF_RECORD_MISC_CPUMODE_UNKNOWN)
    Unknown,
    /// Sample happened in the kernel. (PERF_RECORD_MISC_KERNEL)
    Kernel,
    /// Sample happened in user code. (PERF_RECORD_MISC_USER)
    User,
    /// Sample happened in the hypervisor. (PERF_RECORD_MISC_HYPERVISOR)
    Hypervisor,
    /// Sample happened in the guest kernel. (PERF_RECORD_MISC_GUEST_KERNEL, since Linux 2.6.35)
    GuestKernel,
    /// Sample happened in guest user code. (PERF_RECORD_MISC_GUEST_USER, since Linux 2.6.35)
    GuestUser,
}

impl From<u16> for CpuMode {
    fn from(n: u16) -> Self {
        match n as u32 | PERF_RECORD_MISC_CPUMODE_MASK {
            PERF_RECORD_MISC_CPUMODE_UNKNOWN => CpuMode::Unknown,
            PERF_RECORD_MISC_KERNEL => CpuMode::Kernel,
            PERF_RECORD_MISC_USER => CpuMode::User,
            PERF_RECORD_MISC_HYPERVISOR => CpuMode::Hypervisor,
            PERF_RECORD_MISC_GUEST_KERNEL => CpuMode::GuestKernel,
            PERF_RECORD_MISC_GUEST_USER => CpuMode::GuestUser,
            other => panic!("unrecognized cpu mode: {}", other),
        }
    }
}

//    type   The type value is one of the below.  The values in the corre‐
//           sponding record (that follows the header) depend on the type
//           selected as shown.

//           PERF_RECORD_MMAP
//               The MMAP events record the PROT_EXEC mappings so that we
//               can correlate user-space IPs to code.  They have the fol‐
//               lowing structure:

//                   struct {
//                       struct perf_event_header header;
//                       u32    pid, tid;
//                       u64    addr;
//                       u64    len;
//                       u64    pgoff;
//                       char   filename[];
//                   };

//               pid    is the process ID.

//               tid    is the thread ID.

//               addr   is the address of the allocated memory.  len is the
//                      length of the allocated memory.  pgoff is the page
//                      offset of the allocated memory.  filename is a
//                      string describing the backing of the allocated mem‐
//                      ory.

//           PERF_RECORD_LOST
//               This record indicates when events are lost.

//                   struct {
//                       struct perf_event_header header;
//                       u64    id;
//                       u64    lost;
//                       struct sample_id sample_id;
//                   };

//               id     is the unique event ID for the samples that were
//                      lost.

//               lost   is the number of events that were lost.

//           PERF_RECORD_COMM
//               This record indicates a change in the process name.

//                   struct {
//                       struct perf_event_header header;
//                       u32    pid;
//                       u32    tid;
//                       char   comm[];
//                       struct sample_id sample_id;
//                   };

//               pid    is the process ID.

//               tid    is the thread ID.

//               comm   is a string containing the new name of the process.

//           PERF_RECORD_EXIT
//               This record indicates a process exit event.

//                   struct {
//                       struct perf_event_header header;
//                       u32    pid, ppid;
//                       u32    tid, ptid;
//                       u64    time;
//                       struct sample_id sample_id;
//                   };

//           PERF_RECORD_THROTTLE, PERF_RECORD_UNTHROTTLE
//               This record indicates a throttle/unthrottle event.

//                   struct {
//                       struct perf_event_header header;
//                       u64    time;
//                       u64    id;
//                       u64    stream_id;
//                       struct sample_id sample_id;
//                   };

//           PERF_RECORD_FORK
//               This record indicates a fork event.

//                   struct {
//                       struct perf_event_header header;
//                       u32    pid, ppid;
//                       u32    tid, ptid;
//                       u64    time;
//                       struct sample_id sample_id;
//                   };

//           PERF_RECORD_READ
//               This record indicates a read event.

//                   struct {
//                       struct perf_event_header header;
//                       u32    pid, tid;
//                       struct read_format values;
//                       struct sample_id sample_id;
//                   };

//           PERF_RECORD_SAMPLE
//               This record indicates a sample.

//                   struct {
//                       struct perf_event_header header;
//                       u64    sample_id;   /* if PERF_SAMPLE_IDENTIFIER */
//                       u64    ip;          /* if PERF_SAMPLE_IP */
//                       u32    pid, tid;    /* if PERF_SAMPLE_TID */
//                       u64    time;        /* if PERF_SAMPLE_TIME */
//                       u64    addr;        /* if PERF_SAMPLE_ADDR */
//                       u64    id;          /* if PERF_SAMPLE_ID */
//                       u64    stream_id;   /* if PERF_SAMPLE_STREAM_ID */
//                       u32    cpu, res;    /* if PERF_SAMPLE_CPU */
//                       u64    period;      /* if PERF_SAMPLE_PERIOD */
//                       struct read_format v;
//                                           /* if PERF_SAMPLE_READ */
//                       u64    nr;          /* if PERF_SAMPLE_CALLCHAIN */
//                       u64    ips[nr];     /* if PERF_SAMPLE_CALLCHAIN */
//                       u32    size;        /* if PERF_SAMPLE_RAW */
//                       char  data[size];   /* if PERF_SAMPLE_RAW */
//                       u64    bnr;         /* if PERF_SAMPLE_BRANCH_STACK */
//                       struct perf_branch_entry lbr[bnr];
//                                           /* if PERF_SAMPLE_BRANCH_STACK */
//                       u64    abi;         /* if PERF_SAMPLE_REGS_USER */
//                       u64    regs[weight(mask)];
//                                           /* if PERF_SAMPLE_REGS_USER */
//                       u64    size;        /* if PERF_SAMPLE_STACK_USER */
//                       char   data[size];  /* if PERF_SAMPLE_STACK_USER */
//                       u64    dyn_size;    /* if PERF_SAMPLE_STACK_USER &&
//                                              size != 0 */
//                       u64    weight;      /* if PERF_SAMPLE_WEIGHT */
//                       u64    data_src;    /* if PERF_SAMPLE_DATA_SRC */
//                       u64    transaction; /* if PERF_SAMPLE_TRANSACTION */
//                       u64    abi;         /* if PERF_SAMPLE_REGS_INTR */
//                       u64    regs[weight(mask)];
//                                           /* if PERF_SAMPLE_REGS_INTR */
//                   };

//               sample_id
//                   If PERF_SAMPLE_IDENTIFIER is enabled, a 64-bit unique
//                   ID is included.  This is a duplication of the
//                   PERF_SAMPLE_ID id value, but included at the beginning
//                   of the sample so parsers can easily obtain the value.

//               ip  If PERF_SAMPLE_IP is enabled, then a 64-bit instruc‐
//                   tion pointer value is included.

//               pid, tid
//                   If PERF_SAMPLE_TID is enabled, then a 32-bit process
//                   ID and 32-bit thread ID are included.

//               time
//                   If PERF_SAMPLE_TIME is enabled, then a 64-bit time‐
//                   stamp is included.  This is obtained via local_clock()
//                   which is a hardware timestamp if available and the
//                   jiffies value if not.

//               addr
//                   If PERF_SAMPLE_ADDR is enabled, then a 64-bit address
//                   is included.  This is usually the address of a trace‐
//                   point, breakpoint, or software event; otherwise the
//                   value is 0.

//               id  If PERF_SAMPLE_ID is enabled, a 64-bit unique ID is
//                   included.  If the event is a member of an event group,
//                   the group leader ID is returned.  This ID is the same
//                   as the one returned by PERF_FORMAT_ID.

//               stream_id
//                   If PERF_SAMPLE_STREAM_ID is enabled, a 64-bit unique
//                   ID is included.  Unlike PERF_SAMPLE_ID the actual ID
//                   is returned, not the group leader.  This ID is the
//                   same as the one returned by PERF_FORMAT_ID.

//               cpu, res
//                   If PERF_SAMPLE_CPU is enabled, this is a 32-bit value
//                   indicating which CPU was being used, in addition to a
//                   reserved (unused) 32-bit value.

//               period
//                   If PERF_SAMPLE_PERIOD is enabled, a 64-bit value indi‐
//                   cating the current sampling period is written.

//               v   If PERF_SAMPLE_READ is enabled, a structure of type
//                   read_format is included which has values for all
//                   events in the event group.  The values included depend
//                   on the read_format value used at perf_event_open()
//                   time.

//               nr, ips[nr]
//                   If PERF_SAMPLE_CALLCHAIN is enabled, then a 64-bit
//                   number is included which indicates how many following
//                   64-bit instruction pointers will follow.  This is the
//                   current callchain.

//               size, data[size]
//                   If PERF_SAMPLE_RAW is enabled, then a 32-bit value
//                   indicating size is included followed by an array of
//                   8-bit values of length size.  The values are padded
//                   with 0 to have 64-bit alignment.

//                   This RAW record data is opaque with respect to the
//                   ABI.  The ABI doesn't make any promises with respect
//                   to the stability of its content, it may vary depending
//                   on event, hardware, and kernel version.

//               bnr, lbr[bnr]
//                   If PERF_SAMPLE_BRANCH_STACK is enabled, then a 64-bit
//                   value indicating the number of records is included,
//                   followed by bnr perf_branch_entry structures which
//                   each include the fields:

//                   from   This indicates the source instruction (may not
//                          be a branch).

//                   to     The branch target.

//                   mispred
//                          The branch target was mispredicted.

//                   predicted
//                          The branch target was predicted.

//                   in_tx (since Linux 3.11)
//                          The branch was in a transactional memory trans‐
//                          action.

//                   abort (since Linux 3.11)
//                          The branch was in an aborted transactional mem‐
//                          ory transaction.

//                   cycles (since Linux 4.3)
//                          This reports the number of cycles elapsed since
//                          the previous branch stack update.

//                   The entries are from most to least recent, so the
//                   first entry has the most recent branch.

//                   Support for mispred, predicted, and cycles is
//                   optional; if not supported, those values will be 0.

//                   The type of branches recorded is specified by the
//                   branch_sample_type field.

//               abi, regs[weight(mask)]
//                   If PERF_SAMPLE_REGS_USER is enabled, then the user CPU
//                   registers are recorded.

//                   The abi field is one of PERF_SAMPLE_REGS_ABI_NONE,
//                   PERF_SAMPLE_REGS_ABI_32 or PERF_SAMPLE_REGS_ABI_64.

//                   The regs field is an array of the CPU registers that
//                   were specified by the sample_regs_user attr field.
//                   The number of values is the number of bits set in the
//                   sample_regs_user bit mask.

//               size, data[size], dyn_size
//                   If PERF_SAMPLE_STACK_USER is enabled, then the user
//                   stack is recorded.  This can be used to generate stack
//                   backtraces.  size is the size requested by the user in
//                   sample_stack_user or else the maximum record size.
//                   data is the stack data (a raw dump of the memory
//                   pointed to by the stack pointer at the time of sam‐
//                   pling).  dyn_size is the amount of data actually
//                   dumped (can be less than size).  Note that dyn_size is
//                   omitted if size is 0.

//               weight
//                   If PERF_SAMPLE_WEIGHT is enabled, then a 64-bit value
//                   provided by the hardware is recorded that indicates
//                   how costly the event was.  This allows expensive
//                   events to stand out more clearly in profiles.

//               data_src
//                   If PERF_SAMPLE_DATA_SRC is enabled, then a 64-bit
//                   value is recorded that is made up of the following
//                   fields:

//                   mem_op
//                       Type of opcode, a bitwise combination of:

//                       PERF_MEM_OP_NA          Not available
//                       PERF_MEM_OP_LOAD        Load instruction
//                       PERF_MEM_OP_STORE       Store instruction
//                       PERF_MEM_OP_PFETCH      Prefetch
//                       PERF_MEM_OP_EXEC        Executable code

//                   mem_lvl
//                       Memory hierarchy level hit or miss, a bitwise com‐
//                       bination of the following, shifted left by
//                       PERF_MEM_LVL_SHIFT:

//                       PERF_MEM_LVL_NA         Not available
//                       PERF_MEM_LVL_HIT        Hit
//                       PERF_MEM_LVL_MISS       Miss
//                       PERF_MEM_LVL_L1         Level 1 cache
//                       PERF_MEM_LVL_LFB        Line fill buffer
//                       PERF_MEM_LVL_L2         Level 2 cache
//                       PERF_MEM_LVL_L3         Level 3 cache
//                       PERF_MEM_LVL_LOC_RAM    Local DRAM
//                       PERF_MEM_LVL_REM_RAM1   Remote DRAM 1 hop
//                       PERF_MEM_LVL_REM_RAM2   Remote DRAM 2 hops
//                       PERF_MEM_LVL_REM_CCE1   Remote cache 1 hop
//                       PERF_MEM_LVL_REM_CCE2   Remote cache 2 hops
//                       PERF_MEM_LVL_IO         I/O memory
//                       PERF_MEM_LVL_UNC        Uncached memory

//                   mem_snoop
//                       Snoop mode, a bitwise combination of the follow‐
//                       ing, shifted left by PERF_MEM_SNOOP_SHIFT:

//                       PERF_MEM_SNOOP_NA       Not available
//                       PERF_MEM_SNOOP_NONE     No snoop
//                       PERF_MEM_SNOOP_HIT      Snoop hit
//                       PERF_MEM_SNOOP_MISS     Snoop miss
//                       PERF_MEM_SNOOP_HITM     Snoop hit modified

//                   mem_lock
//                       Lock instruction, a bitwise combination of the
//                       following, shifted left by PERF_MEM_LOCK_SHIFT:

//                       PERF_MEM_LOCK_NA        Not available
//                       PERF_MEM_LOCK_LOCKED    Locked transaction

//                   mem_dtlb
//                       TLB access hit or miss, a bitwise combination of
//                       the following, shifted left by PERF_MEM_TLB_SHIFT:

//                       PERF_MEM_TLB_NA         Not available
//                       PERF_MEM_TLB_HIT        Hit
//                       PERF_MEM_TLB_MISS       Miss
//                       PERF_MEM_TLB_L1         Level 1 TLB
//                       PERF_MEM_TLB_L2         Level 2 TLB
//                       PERF_MEM_TLB_WK         Hardware walker
//                       PERF_MEM_TLB_OS         OS fault handler

//               transaction
//                   If the PERF_SAMPLE_TRANSACTION flag is set, then a
//                   64-bit field is recorded describing the sources of any
//                   transactional memory aborts.

//                   The field is a bitwise combination of the following
//                   values:

//                   PERF_TXN_ELISION
//                          Abort from an elision type transaction (Intel-
//                          CPU-specific).

//                   PERF_TXN_TRANSACTION
//                          Abort from a generic transaction.

//                   PERF_TXN_SYNC
//                          Synchronous abort (related to the reported
//                          instruction).

//                   PERF_TXN_ASYNC
//                          Asynchronous abort (not related to the reported
//                          instruction).

//                   PERF_TXN_RETRY
//                          Retryable abort (retrying the transaction may
//                          have succeeded).

//                   PERF_TXN_CONFLICT
//                          Abort due to memory conflicts with other
//                          threads.

//                   PERF_TXN_CAPACITY_WRITE
//                          Abort due to write capacity overflow.

//                   PERF_TXN_CAPACITY_READ
//                          Abort due to read capacity overflow.

//                   In addition, a user-specified abort code can be
//                   obtained from the high 32 bits of the field by shift‐
//                   ing right by PERF_TXN_ABORT_SHIFT and masking with the
//                   value PERF_TXN_ABORT_MASK.

//               abi, regs[weight(mask)]
//                   If PERF_SAMPLE_REGS_INTR is enabled, then the user CPU
//                   registers are recorded.

//                   The abi field is one of PERF_SAMPLE_REGS_ABI_NONE,
//                   PERF_SAMPLE_REGS_ABI_32, or PERF_SAMPLE_REGS_ABI_64.

//                   The regs field is an array of the CPU registers that
//                   were specified by the sample_regs_intr attr field.
//                   The number of values is the number of bits set in the
//                   sample_regs_intr bit mask.

//           PERF_RECORD_MMAP2
//               This record includes extended information on mmap(2) calls
//               returning executable mappings.  The format is similar to
//               that of the PERF_RECORD_MMAP record, but includes extra
//               values that allow uniquely identifying shared mappings.

//                   struct {
//                       struct perf_event_header header;
//                       u32    pid;
//                       u32    tid;
//                       u64    addr;
//                       u64    len;
//                       u64    pgoff;
//                       u32    maj;
//                       u32    min;
//                       u64    ino;
//                       u64    ino_generation;
//                       u32    prot;
//                       u32    flags;
//                       char   filename[];
//                       struct sample_id sample_id;
//                   };

//               pid    is the process ID.

//               tid    is the thread ID.

//               addr   is the address of the allocated memory.

//               len    is the length of the allocated memory.

//               pgoff  is the page offset of the allocated memory.

//               maj    is the major ID of the underlying device.

//               min    is the minor ID of the underlying device.

//               ino    is the inode number.

//               ino_generation
//                      is the inode generation.

//               prot   is the protection information.

//               flags  is the flags information.

//               filename
//                      is a string describing the backing of the allocated
//                      memory.

//           PERF_RECORD_AUX (since Linux 4.1)

//               This record reports that new data is available in the sep‐
//               arate AUX buffer region.

//                   struct {
//                       struct perf_event_header header;
//                       u64    aux_offset;
//                       u64    aux_size;
//                       u64    flags;
//                       struct sample_id sample_id;
//                   };

//               aux_offset
//                      offset in the AUX mmap region where the new data
//                      begins.

//               aux_size
//                      size of the data made available.

//               flags  describes the AUX update.

//                      PERF_AUX_FLAG_TRUNCATED
//                             if set, then the data returned was truncated
//                             to fit the available buffer size.

//                      PERF_AUX_FLAG_OVERWRITE
//                             if set, then the data returned has overwrit‐
//                             ten previous data.

//           PERF_RECORD_ITRACE_START (since Linux 4.1)

//               This record indicates which process has initiated an
//               instruction trace event, allowing tools to properly corre‐
//               late the instruction addresses in the AUX buffer with the
//               proper executable.

//                   struct {
//                       struct perf_event_header header;
//                       u32    pid;
//                       u32    tid;
//                   };

//               pid    process ID of the thread starting an instruction
//                      trace.

//               tid    thread ID of the thread starting an instruction
//                      trace.

//           PERF_RECORD_LOST_SAMPLES (since Linux 4.2)

//               When using hardware sampling (such as Intel PEBS) this
//               record indicates some number of samples that may have been
//               lost.

//                   struct {
//                       struct perf_event_header header;
//                       u64    lost;
//                       struct sample_id sample_id;
//                   };

//               lost   the number of potentially lost samples.

//           PERF_RECORD_SWITCH (since Linux 4.3)

//               This record indicates a context switch has happened.  The
//               PERF_RECORD_MISC_SWITCH_OUT bit in the misc field indi‐
//               cates whether it was a context switch into or away from
//               the current process.

//                   struct {
//                       struct perf_event_header header;
//                       struct sample_id sample_id;
//                   };

//           PERF_RECORD_SWITCH_CPU_WIDE (since Linux 4.3)

//               As with PERF_RECORD_SWITCH this record indicates a context
//               switch has happened, but it only occurs when sampling in
//               CPU-wide mode and provides additional information on the
//               process being switched to/from.  The
//               PERF_RECORD_MISC_SWITCH_OUT bit in the misc field indi‐
//               cates whether it was a context switch into or away from
//               the current process.

//                   struct {
//                       struct perf_event_header header;
//                       u32 next_prev_pid;
//                       u32 next_prev_tid;
//                       struct sample_id sample_id;
//                   };

//               next_prev_pid
//                      The process ID of the previous (if switching in) or
//                      next (if switching out) process on the CPU.

//               next_prev_tid
//                      The thread ID of the previous (if switching in) or
//                      next (if switching out) thread on the CPU.

pub enum SampledEventType {}

impl From<u32> for SampledEventType {
    fn from(n: u32) -> Self {
        unimplemented!();
    }
}

use std::sync::atomic::{fence, Ordering};

/// When using perf_event_open() in sampled mode, asynchronous events (like counter overflow or
/// PROT_EXEC mmap tracking) are logged into a ring-buffer. This ring-buffer is created and accessed
/// through mmap(2).
pub(crate) struct RingBuffer {
    // SAFETY: this should be before the fd now that rust specifies drop order
    map: MemoryMap,
    file: PerfEventFile,
    // FIXME is this safe to transmute to? we cant run destructors if someone has a borrow into it
    // struct perf_event_mmap_page {
    //     __u32 version;        /* version number of this structure */
    //     __u32 compat_version; /* lowest version this is compat with */
    //     __u32 lock;           /* seqlock for synchronization */
    //     __u32 index;          /* hardware counter identifier */
    //     __s64 offset;         /* add to hardware counter value */
    //     __u64 time_enabled;   /* time event active */
    //     __u64 time_running;   /* time event on CPU */
    //     union {
    //         __u64   capabilities;
    //         struct {
    //             __u64 cap_usr_time / cap_usr_rdpmc / cap_bit0 : 1,
    //                   cap_bit0_is_deprecated : 1,
    //                   cap_user_rdpmc         : 1,
    //                   cap_user_time          : 1,
    //                   cap_user_time_zero     : 1,
    //         };
    //     };
    //     __u16 pmc_width;
    //     __u16 time_shift;
    //     __u32 time_mult;
    //     __u64 time_offset;
    //     __u64 __reserved[120];   /* Pad to 1 k */
    //     __u64 data_head;         /* head in the data section */
    //     __u64 data_tail;         /* user-space written tail */
    //     __u64 data_offset;       /* where the buffer starts */
    //     __u64 data_size;         /* data buffer size */
    //     __u64 aux_head;
    //     __u64 aux_tail;
    //     __u64 aux_offset;
    //     __u64 aux_size;
    // }
    metadata: *mut perf_event_mmap_page,
}

impl RingBuffer {
    /// Creates a new buffer, 8k pages by default.
    ///
    /// TODO(anp): validate this default size in literally any way.
    fn new(file: PerfEventFile) -> Result<Self> {
        Self::with_page_capacity(file, 8192)
    }

    fn with_page_capacity(file: PerfEventFile, num_pages: usize) -> Result<Self> {
        let size = (num_pages + 1) * page_size();
        let map = MemoryMap::new(
            size,
            &[
                MapOption::MapFd(file.0.as_raw_fd()),
                MapOption::MapReadable,
                // setting the writeable flag lets us tell the kernel where we've finished reading
                MapOption::MapWritable,
            ],
        )?;

        let metadata = map.data() as *const _ as *mut perf_event_mmap_page;

        Ok(Self {
            file,
            map,
            metadata,
        })
    }

    // Time the event was active.
    //
    // Note: author of this crate isn't *entirely* sure of the semantics here either.
    // fn time_enabled(&self) -> u64 {
    //     self.metadata.time_enabled
    // }

    // Time the event was running.
    //
    // Note: author of this crate isn't *entirely* sure of the semantics here either.
    // fn time_running(&self) -> u64 {
    //     self.metadata.time_running
    // }

    // cap_user_time (since Linux 3.12)
    //        This bit indicates the hardware has a constant, nonstop time‐
    //        stamp counter (TSC on x86).

    // cap_user_time_zero (since Linux 3.12)
    //        Indicates the presence of time_zero which allows mapping time‐
    //        stamp values to the hardware clock.

    // time_shift, time_mult, time_offset

    //        If cap_usr_time, these fields can be used to compute the time
    //        delta since time_enabled (in nanoseconds) using rdtsc or simi‐
    //        lar.

    //            u64 quot, rem;
    //            u64 delta;
    //            quot = (cyc >> time_shift);
    //            rem = cyc & (((u64)1 << time_shift) - 1);
    //            delta = time_offset + quot * time_mult +
    //                    ((rem * time_mult) >> time_shift);

    //        Where time_offset, time_mult, time_shift, and cyc are read in
    //        the seqcount loop described above.  This delta can then be
    //        added to enabled and possible running (if idx), improving the
    //        scaling:

    //            enabled += delta;
    //            if (idx)
    //                running += delta;
    //            quot = count / running;
    //            rem  = count % running;
    //            count = quot * enabled + (rem * enabled) / running;

    // time_zero (since Linux 3.12)

    //        If cap_usr_time_zero is set, then the hardware clock (the TSC
    //        timestamp counter on x86) can be calculated from the
    //        time_zero, time_mult, and time_shift values:

    //            time = timestamp - time_zero;
    //            quot = time / time_mult;
    //            rem  = time % time_mult;
    //            cyc = (quot << time_shift) + (rem << time_shift) / time_mult;

    //        And vice versa:

    //            quot = cyc >> time_shift;
    //            rem  = cyc & (((u64)1 << time_shift) - 1);
    //            timestamp = time_zero + quot * time_mult +
    //                ((rem * time_mult) >> time_shift);

    unsafe fn slices(&self, offset: usize, len: usize) -> (&[u8], &[u8]) {
        // ms = mmap_start
        // ml = mmap_len
        // s = start
        //   always greater than mmap_start (offset is unsigned)
        // l = len
        //

        // contiguous
        // ms---------s-------------------sl------ml

        // contiguous ending at end of mmap
        // ms--------------------s-------------sl|ml

        // split
        // ms---------sl-------------------s------ml

        let mmap_start = self.map.data();
        let mmap_len = self.map.len();
        let mmap_end = mmap_start.offset(mmap_len as isize);

        let start = mmap_start.offset(offset as isize);
        let natural_first_end = start.offset(len as isize);
        let (first_len, second_len) = if natural_first_end < mmap_end {
            (len, 0)
        } else {
            let first_len = mmap_len - offset;
            (first_len, len - first_len)
        };

        assert_eq!(first_len + second_len, len);

        let first = ::std::slice::from_raw_parts(start, first_len);

        // i'm fairly confident we wont have this issue but the docs aren't super clear
        let second =
            ::std::slice::from_raw_parts(mmap_start.offset(page_size() as isize), second_len);

        (first, second)
    }

    fn head_offset_len(&self) -> (usize, usize, usize) {
        // This points to the head of the data section.  The value con‐
        // tinuously increases, it does not wrap.  The value needs to be
        // manually wrapped by the size of the mmap buffer before access‐
        // ing the samples.

        // On SMP-capable platforms, after reading the data_head value,
        // user space should issue an rmb().
        let head = unsafe { (*self.metadata).data_head } as usize % self.map.len();

        // DOCS(anp): need to document this minimum kernel version requirement
        // data_offset (since Linux 4.1)
        //        Contains the offset of the location in the mmap buffer where
        //        perf sample data begins.
        let offset = unsafe { (*self.metadata).data_offset } as usize;

        assert!(offset <= head);

        // data_size (since Linux 4.1)
        //        Contains the size of the perf sample region within the mmap
        //        buffer.
        let len = unsafe { (*self.metadata).data_size } as usize;

        fence(Ordering::Acquire); // i *think* this corresponds to rmb() (lfence on x86)

        (head, offset, len)
    }

    /// When the mapping is PROT_WRITE, the data_tail value should be written by user space to
    /// reflect the last read data.  In this case, the kernel will not overwrite unread data.
    fn set_tail(&mut self, new_tail: usize) {
        // NOTE(anp): we guarantee PROT_WRITE in our constructors
        fence(Ordering::Release); // i *think* this corresponds to mb() (mfence on x86)
        unsafe {
            (*self.metadata).data_tail = new_tail as u64;
        }
    }
}

impl Read for RingBuffer {
    fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
        let (head, offset, len) = self.head_offset_len();

        // we might only get to copy some portion of the bytes
        let len = len.min(buf.len());

        {
            let (first_src, second_src) = unsafe { self.slices(offset, len) };
            let (first_dest, second_dest) = (&mut buf[..len]).split_at_mut(first_src.len());

            first_dest.copy_from_slice(first_src);
            second_dest.copy_from_slice(second_src);
        }

        // The following 2^n ring-buffer pages have the layout described below.

        // If perf_event_attr.sample_id_all is set, then all event types will
        // have the sample_type selected fields related to where/when (identity)
        // an event took place (TID, TIME, ID, CPU, STREAM_ID) described in
        // PERF_RECORD_SAMPLE below, it will be stashed just after the
        // perf_event_header and the fields already present for the existing
        // fields, that is, at the end of the payload.  This allows a newer
        // perf.data file to be supported by older perf tools, with the new
        // optional fields being ignored.

        self.set_tail(head);

        // TODO(anp): figure out how we need to use these

        // version
        //        Version number of this structure.

        // compat_version
        //        The lowest version this is compatible with.

        // lock   A seqlock for synchronization.

        // index  A unique hardware counter identifier.

        // offset When using rdpmc for reads this offset value must be added to
        //        the one returned by rdpmc to get the current total event
        //        count.

        // aux_head, aux_tail, aux_offset, aux_size (since Linux 4.1)
        //        The AUX region allows mmaping a separate sample buffer for
        //        high-bandwidth data streams (separate from the main perf sam‐
        //        ple buffer).  An example of a high-bandwidth stream is
        //        instruction tracing support, as is found in newer Intel pro‐
        //        cessors.

        //        To set up an AUX area, first aux_offset needs to be set with
        //        an offset greater than data_offset+data_size and aux_size
        //        needs to be set to the desired buffer size.  The desired off‐
        //        set and size must be page aligned, and the size must be a
        //        power of two.  These values are then passed to mmap in order
        //        to map the AUX buffer.  Pages in the AUX buffer are included
        //        as part of the RLIMIT_MEMLOCK resource limit (see
        //        setrlimit(2)), and also as part of the perf_event_mlock_kb
        //        allowance.

        //        By default, the AUX buffer will be truncated if it will not
        //        fit in the available space in the ring buffer.  If the AUX
        //        buffer is mapped as a read only buffer, then it will operate
        //        in ring buffer mode where old data will be overwritten by new.
        //        In overwrite mode, it might not be possible to infer where the
        //        new data began, and it is the consumer's job to disable mea‐
        //        surement while reading to avoid possible data races.

        //        The aux_head and aux_tail ring buffer pointers have the same
        //        behavior and ordering rules as the previous described
        //        data_head and data_tail.

        // cap_bit0_is_deprecated (since Linux 3.12)
        //        If set, this bit indicates that the kernel supports the prop‐
        //        erly separated cap_user_time and cap_user_rdpmc bits.

        //        If not-set, it indicates an older kernel where cap_usr_time
        //        and cap_usr_rdpmc map to the same bit and thus both features
        //        should be used with caution.

        // cap_user_rdpmc (since Linux 3.12)
        //        If the hardware supports user-space read of performance coun‐
        //        ters without syscall (this is the "rdpmc" instruction on x86),
        //        then the following code can be used to do a read:

        //            u32 seq, time_mult, time_shift, idx, width;
        //            u64 count, enabled, running;
        //            u64 cyc, time_offset;

        //            do {
        //                seq = pc->lock;
        //                barrier();
        //                enabled = pc->time_enabled;
        //                running = pc->time_running;

        //                if (pc->cap_usr_time && enabled != running) {
        //                    cyc = rdtsc();
        //                    time_offset = pc->time_offset;
        //                    time_mult   = pc->time_mult;
        //                    time_shift  = pc->time_shift;
        //                }

        //                idx = pc->index;
        //                count = pc->offset;

        //                if (pc->cap_usr_rdpmc && idx) {
        //                    width = pc->pmc_width;
        //                    count += rdpmc(idx - 1);
        //                }

        //                barrier();
        //            } while (pc->lock != seq);

        // cap_usr_time / cap_usr_rdpmc / cap_bit0 (since Linux 3.4)
        //        There was a bug in the definition of cap_usr_time and
        //        cap_usr_rdpmc from Linux 3.4 until Linux 3.11.  Both bits were
        //        defined to point to the same location, so it was impossible to
        //        know if cap_usr_time or cap_usr_rdpmc were actually set.

        //        Starting with Linux 3.12, these are renamed to cap_bit0 and
        //        you should use the cap_user_time and cap_user_rdpmc fields
        //        instead.

        // pmc_width
        //        If cap_usr_rdpmc, this field provides the bit-width of the
        //        value read using the rdpmc or equivalent instruction.  This
        //        can be used to sign extend the result like:

        //            pmc <<= 64 - pmc_width;
        //            pmc >>= 64 - pmc_width; // signed shift right
        //            count += pmc;
        Ok(len)
    }
}

impl Evented for RingBuffer {
    fn register(&self, poll: &Poll, token: Token, interest: Ready, opts: PollOpt) -> IoResult<()> {
        EventedFd(&self.file.0.as_raw_fd()).register(poll, token, interest, opts)
    }

    fn reregister(
        &self,
        poll: &Poll,
        token: Token,
        interest: Ready,
        opts: PollOpt,
    ) -> IoResult<()> {
        EventedFd(&self.file.0.as_raw_fd()).reregister(poll, token, interest, opts)
    }

    fn deregister(&self, poll: &Poll) -> IoResult<()> {
        EventedFd(&self.file.0.as_raw_fd()).deregister(poll)
    }
}

//    Overflow handling
//        Events can be set to notify when a threshold is crossed, indicating
//        an overflow.  Overflow conditions can be captured by monitoring the
//        event file descriptor with poll(2), select(2), or epoll(7).  Alterna‐
//        tively, the overflow events can be captured via sa signal handler, by
//        enabling I/O signaling on the file descriptor; see the discussion of
//        the F_SETOWN and F_SETSIG operations in fcntl(2).

//        Overflows are generated only by sampling events (sample_period must
//        have a nonzero value).

//        There are two ways to generate overflow notifications.

//        The first is to set a wakeup_events or wakeup_watermark value that
//        will trigger if a certain number of samples or bytes have been writ‐
//        ten to the mmap ring buffer.  In this case, POLL_IN is indicated.

//        The other way is by use of the PERF_EVENT_IOC_REFRESH ioctl.  This
//        ioctl adds to a counter that decrements each time the event over‐
//        flows.  When nonzero, POLL_IN is indicated, but once the counter
//        reaches 0 POLL_HUP is indicated and the underlying event is disabled.

//        Refreshing an event group leader refreshes all siblings and refresh‐
//        ing with a parameter of 0 currently enables infinite refreshes; these
//        behaviors are unsupported and should not be relied on.

//        Starting with Linux 3.18, POLL_HUP is indicated if the event being
//        monitored is attached to a different process and that process exits.
