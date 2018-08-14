use std::fmt::Debug;
use std::fs::File;
use std::io;
use std::io::Read;
use std::io::Result as IoResult;
use std::ops::{Deref, DerefMut};
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};

use libc::{syscall, SYS_perf_event_open};
use mio::{unix::EventedFd, Evented, Poll, PollOpt, Ready, Token};
use nix::errno::Errno;

use super::EventConfig;
use error::*;
use raw::perf_event_attr;

pub trait PerfEventAttrThingy {
    fn apply(&self, &mut perf_event_attr);
}

#[derive(Debug)]
pub struct PerfFile(pub(crate) File);

impl PerfFile {
    pub fn new<A: Debug + PerfEventAttrThingy>(a: A, config: EventConfig) -> Result<Self> {
        // pub(crate) fn as_raw(&self, disabled: bool) -> perf_event_attr {
        // NOTE(unsafe) a zeroed struct is what the example c code uses,
        // zero fields are interpreted as "off" afaict, aside from the required fields
        let attr = config.raw();

        // NOTE(unsafe) it'd be a kernel bug if this caused unsafety, i think
        unsafe {
            let res = syscall(
                SYS_perf_event_open,
                &attr,
                config.pid.raw(),
                config.cpu.raw(),
                // ignore group_fd, since we can't set inherit *and* read multiple from a group
                -1,
                // NOTE: doesnt seem like this is needed for this library, but
                // i could be wrong. CLOEXEC doesn't seem to apply when we won't
                // leak the file descriptor, NO_GROUP doesn't make since FD_OUTPUT
                // has been broken since 2.6.35, and PID_CGROUP isn't useful
                // unless you're running inside containers, which i don't need to
                // support yet
                0,
            );

            if res == -1 {
                let e = Error::from(OpenError::from(Errno::last()));
                debug!("unable to open {:?}: {:?}", a, e);
                Err(e)
            } else {
                // NOTE(unsafe) if the kernel doesn't give -1, guarantees the fd is valid
                let f = File::from_raw_fd(res as i32);
                Ok(PerfFile(f))
            }
        }
    }

    pub fn enable(&self) -> Result<()> {
        const PERF_EVENT_IOC_MAGIC: u8 = b'$';
        const PERF_EVENT_IOC_ENABLE_MODE: u8 = 0;

        ioctl!(
            none
            perf_event_ioc_enable
            with
            PERF_EVENT_IOC_MAGIC,
            PERF_EVENT_IOC_ENABLE_MODE
        );

        unsafe {
            perf_event_ioc_enable(self.0.as_raw_fd())
                .map(|_| ())
                .map_err(|e| {
                    warn!("Unable to enable a pe file descriptor: {:?}", e);
                    Error::Posix { inner: e }
                })
        }
    }
}

impl Evented for PerfFile {
    fn register(&self, poll: &Poll, token: Token, interest: Ready, opts: PollOpt) -> IoResult<()> {
        EventedFd(&self.0.as_raw_fd()).register(poll, token, interest, opts)
    }

    fn reregister(
        &self,
        poll: &Poll,
        token: Token,
        interest: Ready,
        opts: PollOpt,
    ) -> IoResult<()> {
        EventedFd(&self.0.as_raw_fd()).reregister(poll, token, interest, opts)
    }

    fn deregister(&self, poll: &Poll) -> IoResult<()> {
        EventedFd(&self.0.as_raw_fd()).deregister(poll)
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

impl Read for PerfFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

impl AsRawFd for PerfFile {
    fn as_raw_fd(&self) -> RawFd {
        self.0.as_raw_fd()
    }
}

impl Deref for PerfFile {
    type Target = File;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for PerfFile {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
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
