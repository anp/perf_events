use std::fs::File;
use std::io;
use std::io::Read;
use std::ops::{Deref, DerefMut};
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};

use libc::{syscall, SYS_perf_event_open};
use nix::errno::Errno;

use super::{CpuConfig, PidConfig};
use events::Event;

pub fn create_fd(event: Event, pid: PidConfig, cpu: CpuConfig) -> Result<PerfEventFile, OpenError> {
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
            -1 => Err(Errno::last().into()),
            // NOTE(unsafe) if the kernel doesn't give -1, guarantees the fd is valid
            fd => Ok(PerfEventFile(File::from_raw_fd(fd as i32), event)),
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

#[derive(Debug)]
pub struct PerfEventFile(pub(crate) File, pub(crate) Event);

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
