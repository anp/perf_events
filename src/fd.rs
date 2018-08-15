use std::fmt::Debug;
use std::fs::File;
use std::io;
use std::io::Error as IoError;
use std::io::Read;
use std::io::Result as IoResult;
use std::ops::{Deref, DerefMut};
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};

use libc::*;
use mio::{unix::EventedFd, Evented, Poll, PollOpt, Ready, Token};
use nix::errno::errno;
use nix::errno::Errno;

use super::{CpuConfig, PidConfig};
use error::*;
use raw::perf_event_attr;

pub trait PerfEventAttrThingy {
    fn apply(&self, &mut perf_event_attr);
}

#[derive(Debug)]
pub struct PerfFile(pub(crate) File);

impl PerfFile {
    pub fn new(
        config: impl Debug + Into<perf_event_attr> + AsRef<PidConfig> + AsRef<CpuConfig>,
    ) -> Result<Self> {
        // pub(crate) fn as_raw(&self, disabled: bool) -> perf_event_attr {
        // NOTE(unsafe) a zeroed struct is what the example c code uses,
        // zero fields are interpreted as "off" afaict, aside from the required fields
        let pid: PidConfig = *config.as_ref();
        let cpu: CpuConfig = *config.as_ref();
        let attr = config.into();

        // NOTE(unsafe) it'd be a kernel bug if this caused unsafety, i think
        unsafe {
            let res = syscall(
                SYS_perf_event_open,
                &attr,
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
            );

            if res == -1 {
                let e = Error::from(OpenError::from(Errno::last()));
                debug!("unable to open {:?}: {:?}", attr, e);
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
        info!("registering {:?}", self.0);

        // before we try to mmap this, we need to make sure it's in async mode!
        // i think this is done by registering with mio?
        // if 0 != unsafe { libc::fcntl(fd, F_SETSIG, libc::SIGIO) } {
        //     let e = errno();
        //     return Err(FileControlError::from_i32(e).unwrap().into());
        // }

        //The F_SETOWN_EX option to fcntl(2) is needed to properly get overflow
        //    signals in threads.  This was introduced in Linux 2.6.32.
        #[repr(C)]
        struct FOwnerEx(c_int, pid_t);

        info!("getting thread id");
        let owner = FOwnerEx(F_OWNER_TID, unsafe { syscall(SYS_gettid) as pid_t });

        let fd = self.0.as_raw_fd();

        info!("setting recipient thread for overflow notifs");
        if 0 != unsafe { fcntl(fd, F_SETOWN_EX, &owner) } {
            return Err(IoError::from_raw_os_error(errno()));
        }

        if 0 != unsafe { fcntl(fd, F_SETFL, O_ASYNC | O_NONBLOCK | O_RDONLY) } {
            return Err(IoError::from_raw_os_error(errno()));
        }

        info!("registering our file descriptor with the event loop");
        EventedFd(&self.0.as_raw_fd()).register(poll, token, interest, opts)
    }

    fn reregister(
        &self,
        poll: &Poll,
        token: Token,
        interest: Ready,
        opts: PollOpt,
    ) -> IoResult<()> {
        info!("reregistering {:?}", self.0);
        EventedFd(&self.0.as_raw_fd()).reregister(poll, token, interest, opts)
    }

    fn deregister(&self, poll: &Poll) -> IoResult<()> {
        info!("deregistering {:?}", self.0);
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

enum_from_primitive! {
#[repr(i32)]
#[derive(Debug, Fail)]
pub enum FileControlError {
    #[fail(display = "Operation is prohibited by locks held by other processes.")]
    Prohibited = EACCES,

    #[fail(
        display = "The operation is prohibited because the file has been memory-mapped by another
        process."
    )]
    MappedByAnother = EAGAIN,

    #[fail(
        display = "fd is not an open file descriptor.

        -or-

        cmd is F_SETLK or F_SETLKW and the file descriptor open mode doesn't match with the type of
        lock requested."
    )]
    BadFd = EBADF,

    #[fail(
        display = "cmd is F_SETPIPE_SZ and the new pipe capacity specified in arg is smaller than
        the amount of buffer space currently used to store data in the pipe.

        -or-

        cmd is F_ADD_SEALS, arg includes F_SEAL_WRITE, and there exists a writable, shared mapping
        on the file referred to by fd."
    )]
    Busy = EBUSY,

    #[fail(
        display = "It was detected that the specified F_SETLKW command would cause a deadlock."
    )]
    WouldDeadlock = EDEADLK,

    #[fail(display = "lock is outside your accessible address space.")]
    Unaddressable = EFAULT,

    #[fail(
        display = "cmd is F_SETLKW or F_OFD_SETLKW and the operation was interrupted by a signal;
        see signal(7).
        -or-
        cmd is F_GETLK, F_SETLK, F_OFD_GETLK, or F_OFD_SETLK, and the operation was interrupted by a
        signal before the lock was checked or acquired. Most likely when locking a remote file
        (e.g., locking over NFS), but can sometimes happen locally."
    )]
    Interrupted = EINTR,

    #[fail(
        display = "The value specified in cmd is not recognized by this kernel.

        -or-

        cmd is F_ADD_SEALS and arg includes an unrecognized sealing bit.

        -or-

        cmd is F_ADD_SEALS or F_GET_SEALS and the filesystem containing the inode referred to by fd
        does not support sealing.

        -or-

        cmd is F_DUPFD and arg is negative or is greater than the maximum allowable value (see the
        discussion of RLIMIT_NOFILE in getrlimit(2)).

        -or-

        cmd is F_SETSIG and arg is not an allowable signal number.

        -or-

        cmd is F_OFD_SETLK, F_OFD_SETLKW, or F_OFD_GETLK, and l_pid was not specified as zero."
    )]
    InsertCowboyBebopReferenceHereBecauseItsEinval = EINVAL,

    #[fail(
        display = "cmd is F_DUPFD and the per-process limit on the number of open file descriptors
        has been reached."
    )]
    TooManyOpenFiles = EMFILE,

    #[fail(
        display = "Too many segment locks open, lock table is full, or a remote locking protocol
        failed (e.g., locking over NFS)."
    )]
    LockingFailed = ENOLCK,

    #[fail(display = "F_NOTIFY was specified in cmd, but fd does not refer to a directory.")]
    NotADirectory = ENOTDIR,

    #[fail(
        display = "cmd is F_SETPIPE_SZ and the soft or hard user pipe limit has been reached; see
        pipe(7).

        -or-

        Attempted to clear the O_APPEND flag on a file that has the append-only attribute set.

        -or-

        cmd was F_ADD_SEALS, but fd was not open for writing or the current set of seals on the file
        already includes F_SEAL_SEAL."
    )]
    SeveralMiscellaneousErrors = EPERM,
}
}

// https://github.com/torvalds/linux/blob/master/include/uapi/asm-generic/fcntl.h#L115
// #define F_SETSIG	10	/* for sockets. */
const F_SETSIG: i32 = 10;
// #define F_OWNER_TID 0
const F_OWNER_TID: c_int = 0;
// #define F_SETOWN_EX 15
const F_SETOWN_EX: c_int = 15;
