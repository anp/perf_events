use std::io::Read;
use std::io::Result as IoResult;
use std::os::unix::io::AsRawFd;
use std::sync::atomic::{fence, Ordering};

use enum_primitive::FromPrimitive;
use futures::prelude::*;
use libc;
use mio::Ready;
use nix::errno::errno;
use page_size::get as page_size;
use tokio::reactor::PollEvented2;

use super::{config::SamplingConfig, EventConfig};
use error::*;
use fd::PerfFile;
use raw::*;

/// When using perf_event_open() in sampled mode, asynchronous events (like counter overflow or
/// PROT_EXEC mmap tracking) are logged into a ring-buffer. This ring-buffer is created and accessed
/// through mmap(2).
pub(crate) struct RingBuffer {
    poller: PollEvented2<PerfFile>,
    // SAFETY: this should be before the fd now that rust specifies drop order
    base: *mut libc::c_void,
    len: usize,
    prev: Option<u64>,
    start: Option<u64>,
    end: Option<u64>,
    overwrite: bool,
    metadata: *mut perf_event_mmap_page,
}

impl Stream for RingBuffer {
    type Item = Vec<u8>;
    type Error = Error;

    fn poll(&mut self) -> Result<Async<Option<Vec<u8>>>> {
        if let Async::Ready(_) = self.poller.poll_read_ready(Ready::readable())? {
            debug!("fd is ready, dumping to buffer");

            let read = self.read_chunk()?;

            self.poller.clear_read_ready(Ready::readable())?;

            Ok(Async::Ready(Some(read)))
        } else {
            Ok(Async::NotReady)
        }
    }
}

enum State {
    NotReady,
    Running,
    DataPending,
    Empty,
}

use nix::fcntl::{fcntl, FcntlArg, OFlag};
use std::mem::size_of;

impl RingBuffer {
    /// Creates a new buffer, 8k pages by default.
    ///
    /// TODO(anp): validate this default size in literally any way.
    pub fn new(sample_config: SamplingConfig, event_config: EventConfig) -> Result<Self> {
        Self::with_page_capacity(sample_config, event_config, 8192)
    }

    pub fn enable_fd(&self) -> Result<()> {
        self.poller.get_ref().enable()
    }

    fn with_page_capacity(
        sample_config: SamplingConfig,
        event_config: EventConfig,
        pages: usize,
    ) -> Result<Self> {
        let len = (pages + 1) * page_size();
        // FIXME(anp): this should return an Err
        assert!(pages != 0 && (pages & (pages - 1)) == 0);
        // make sure we're aligned on a page boundary for the length we request
        assert!(len % page_size() == 0);

        let file = PerfFile::new(sample_config, event_config)?;

        let fd = file.0.as_raw_fd();

        // before we try to mmap this, we need to make sure it's in async mode!
        fcntl(fd, FcntlArg::F_SETFL(OFlag::O_ASYNC | OFlag::O_NONBLOCK))?;

        let base = unsafe {
            libc::mmap(
                ::std::ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            )
        };

        if base == libc::MAP_FAILED {
            Err(BufferError::from_i32(errno()).unwrap())?
        }

        let metadata = base as *const _ as *mut perf_event_mmap_page;

        Ok(Self {
            poller: PollEvented2::new(file),
            base,
            metadata,
            len,
            prev: None,
            end: None,
            start: None,
            overwrite: false,
        })
    }

    pub fn read_chunk(&mut self) -> Result<Vec<u8>> {
        unimplemented!();
    }

    /// This points to the head of the data section. The value continuously increases, it does not
    /// wrap. The value needs to be manually wrapped by the size of the mmap buffer before accessing
    /// the samples. On SMP-capable platforms, after reading the data_head value, user space should
    /// issue an rmb().
    fn head(&self) -> u64 {
        let head = unsafe { (*self.metadata).data_head };
        fence(Ordering::Acquire); // i *think* this corresponds to rmb() (lfence on x86)
        head
    }

    /// Contains the size of the perf sample region within the mmap buffer. (since Linux 4.1)
    fn size(&self) -> u64 {
        unsafe { (*self.metadata).data_size }
    }

    /// Contains the offset of the location in the mmap buffer where perf sample data begins.
    fn offset(&self) -> u64 {
        // DOCS(anp): need to document this minimum kernel version requirement
        // data_offset (since Linux 4.1)
        unsafe { (*self.metadata).data_offset }
    }

    /// When the mapping is PROT_WRITE, the data_tail value should be written by user space to
    /// reflect the last read data.  In this case, the kernel will not overwrite unread data.
    fn set_tail(&mut self, new_tail: u64) {
        // NOTE(anp): we guarantee PROT_WRITE in our constructors
        fence(Ordering::Release); // i *think* this corresponds to mb() (mfence on x86)
        unsafe {
            (*self.metadata).data_tail = new_tail as u64;
        }
    }
}

impl RingBuffer {
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

    //     unsafe fn slices(&self, offset: usize, len: usize) -> (&[u8], &[u8]) {
    //         // ms = mmap_start
    //         // ml = mmap_len
    //         // s = start
    //         //   always greater than mmap_start (offset is unsigned)
    //         // l = len
    //         //

    //         // contiguous
    //         // ms---------s-------------------sl------ml

    //         // contiguous ending at end of mmap
    //         // ms--------------------s-------------sl|ml

    //         // split
    //         // ms---------sl-------------------s------ml

    //         let mmap_end = self.base.offset(self.len() as isize);

    //         let start = self.base.offset(offset as isize);
    //         let natural_first_end = start.offset(len as isize);
    //         let (first_len, second_len) = if natural_first_end < mmap_end {
    //             (len, 0)
    //         } else {
    //             let first_len = self.len() - offset;
    //             (first_len, len - first_len)
    //         };

    //         assert_eq!(first_len + second_len, len);

    //         let first = ::std::slice::from_raw_parts(start as *mut u8, first_len);

    //         // i'm fairly confident we wont have this issue but the docs aren't super clear
    //         let second = ::std::slice::from_raw_parts(
    //             self.base.offset(page_size() as isize) as *mut u8,
    //             second_len,
    //         );

    //         (first, second)
    //     }
}

impl ::std::ops::Drop for RingBuffer {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.base, self.len);
        }
    }
}

enum_from_primitive! {
#[repr(i32)]
#[derive(Debug, Fail)]
pub enum BufferError {
    #[fail(
        display = "A file descriptor refers to a non-regular file.  Or a file mapping was requested,
        but fd is not open for reading.  Or MAP_SHARED was requested and PROT_WRITE is set, but fd
        is not open in read/write (O_RDWR) mode.  Or PROT_WRITE is set, but the file is append-only."
    )]
    Access = libc::EACCES,

    #[fail(display = "fd is not a valid file descriptor (and MAP_ANONYMOUS was not set).")]
    FdBad = libc::EBADF,

    #[fail(
        display = "We don't like addr, length, or offset (e.g., they are too large, or not aligned
        on a page boundary). length was 0. flags contained neither MAP_PRIVATE or MAP_SHARED, or
        contained both of these values."
    )]
    InvalidArgs = libc::EINVAL,

    #[fail(
        display = "The underlying filesystem of the specified file does not support memory mapping."
    )]
    NoMapSupport = libc::ENODEV,

    #[fail(
        display = "No memory is available. -or-

        The process's maximum number of mappings would have been exceeded. This error can also
        occur for munmap(), when unmapping a region in the middle of an existing mapping, since this
        results in two smaller mappings on either side of the region being unmapped. -or-

        (since Linux 4.7) The process's RLIMIT_DATA limit, described in getrlimit(2), would have
        been exceeded."
    )]
    NoMemory = libc::ENOMEM,

    #[fail(
        display = "The file has been locked, or too much memory has been locked (see setrlimit(2))."
    )]
    TooMuchLocking = libc::EAGAIN,


    #[fail(
        display = "MAP_FIXED_NOREPLACE was specified in flags, and the range covered by addr and
        length is clashes with an existing mapping."
    )]
    ClashesWithExisting = libc::EEXIST,

    #[fail(display = "The system-wide limit on the total number of open files has been reached.")]
    TooManyOpenFiles = libc::ENFILE,

    #[fail(
        display = "On 32-bit architecture together with the large file extension (i.e., using 64-bit
        off_t): the number of pages used for length plus number of pages used for offset would
        overflow unsigned long (32 bits)."
    )]
    Overflow = libc::EOVERFLOW,

    #[fail(
        display = "The prot argument asks for PROT_EXEC but the mapped area belongs to a file on a
        filesystem that was mounted no-exec. -or-

        The operation was prevented by a file seal; see fcntl(2)."
    )]
    ExecFailed = libc::EPERM,

    #[fail(
        display = "MAP_DENYWRITE was set but the object specified by fd is open for writing."
    )]
    DenyWriteFailed = libc::ETXTBSY,
}
}

impl Read for RingBuffer {
    fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
        let head = self.head();

        // we might only get to copy some portion of the bytes
        let len = self.len.min(buf.len());

        // {
        //     let (first_src, second_src) = unsafe { self.slices(offset as usize, len as usize) };
        //     let (first_dest, second_dest) =
        //         (&mut buf[..len as usize]).split_at_mut(first_src.len());

        //     first_dest.copy_from_slice(first_src);
        //     second_dest.copy_from_slice(second_src);
        // }

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
        Ok(len as usize)
    }
}

unsafe impl Send for RingBuffer {}
