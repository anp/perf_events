use std::{
    borrow::Cow,
    mem::size_of,
    os::unix::io::AsRawFd,
    sync::atomic::{fence, Ordering},
};

use enum_primitive::FromPrimitive;
use futures::prelude::*;
use libc;
use mio::Ready;
use nix::errno::errno;
use page_size::get as page_size;
use tokio::reactor::PollEvented2;

use super::{
    config::SamplingConfig,
    record::{EventHeader, Record},
};
use error::*;
use fd::PerfFile;
use raw::*;

/// When using perf_event_open() in sampled mode, asynchronous events (like counter overflow or
/// PROT_EXEC mmap tracking) are logged into a ring-buffer. This ring-buffer is created and accessed
/// through mmap(2).
pub(crate) struct RingBuffer {
    poller: PollEvented2<PerfFile>,
    base: *mut libc::c_void,
    len: usize,
    metadata: *mut perf_event_mmap_page,
    prev: usize,
    start: usize,
    end: usize,
    // interval_started: bool,
}

impl RingBuffer {
    const DEFAULT_PAGES: usize = 128;

    /// Creates a new buffer, 8k pages by default.
    ///
    /// TODO(anp): validate this default size in literally any way.
    pub fn new(sample_config: SamplingConfig) -> Result<Self> {
        Self::with_page_capacity(sample_config, Self::DEFAULT_PAGES)
    }

    pub fn enable_fd(&self) -> Result<()> {
        self.poller.get_ref().enable()
    }

    fn with_page_capacity(sample_config: SamplingConfig, pages: usize) -> Result<Self> {
        let len = (pages + 1) * page_size();
        // FIXME(anp): this should return an Err
        assert!(pages != 0 && (pages & (pages - 1)) == 0);
        // make sure we're aligned on a page boundary for the length we request
        assert!(len % page_size() == 0);

        let file = PerfFile::new(sample_config)?;

        let fd = file.0.as_raw_fd();

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
            prev: 0,
            end: 0,
            start: 0,
        })
    }

    pub fn is_empty(&self) -> bool {
        // 	TODO handle aux map;
        self.head() == self.prev
    }

    /// This points to the head of the data section. The value continuously increases, it does not
    /// wrap. The value needs to be manually wrapped by the size of the mmap buffer before accessing
    /// the samples. On SMP-capable platforms, after reading the data_head value, user space should
    /// issue an rmb().
    fn head(&self) -> usize {
        let head = unsafe { (*self.metadata).data_head };
        fence(Ordering::Acquire); // i *think* this corresponds to rmb() (lfence on x86)
        head as usize
    }

    /// Contains the size of the perf sample region within the mmap buffer. (since Linux 4.1)
    fn size(&self) -> usize {
        unsafe { (*self.metadata).data_size as usize }
    }

    /// Contains the offset of the location in the mmap buffer where perf sample data begins.
    fn offset(&self) -> usize {
        // DOCS(anp): need to document this minimum kernel version requirement
        // data_offset (since Linux 4.1)
        unsafe { (*self.metadata).data_offset as usize }
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

impl Stream for RingBuffer {
    type Item = Record;
    type Error = Error;

    fn poll(&mut self) -> Result<Async<Option<Self::Item>>> {
        // if !self.interval_started {
        //     let handle = ::tokio::prelude::task::current();
        //     let timer = Interval::new(Instant::now(), Duration::from_secs(2))
        //         .map(move |_| {
        //             trace!("refreshing sampler readiness");
        //             handle.notify();
        //         })
        //         .map_err(|e| panic!("timer error: {:?}", e))
        //         .for_each(|()| ::futures::future::ok(()));

        //     ::tokio::spawn(timer);
        // }

        trace!("ring buffer polled");
        let res = if let Async::Ready(_) = self.poller.poll_read_ready(Ready::readable())? {
            info!("file descriptor was ready, parsing records");
            self.next()
        } else {
            None
        };

        trace!("clearing fd readiness");
        self.poller.clear_read_ready(Ready::readable())?;

        if let Some(r) = res {
            Ok(Async::Ready(Some(r?)))
        } else {
            Ok(Async::NotReady)
        }
    }
}

impl Iterator for RingBuffer {
    type Item = Result<Record>;

    fn next(&mut self) -> Option<Self::Item> {
        trace!("next record...");
        let (header, event_bytes) = self.next_event_bytes()?;
        info!("parsing record");
        Some(Record::from_slice(header, &event_bytes))
    }
}

impl RingBuffer {
    fn next_event_bytes(&mut self) -> Option<(EventHeader, Cow<[u8]>)> {
        let header_size = size_of::<perf_event_header>();
        unsafe {
            self.end = self.head();

            assert!(
                self.end >= self.start,
                "we wrapped around and we dont support that yet lol"
            );

            let diff = self.end - self.start;

            if diff < header_size {
                debug!("gap between start and end is too small for a header");
                return None;
            }

            let data = self.base.offset(page_size() as isize);

            let raw_header: &perf_event_header =
                &*(data.offset(self.start as isize) as *const perf_event_header);
            let header = ::sample::record::EventHeader::from(raw_header);
            let event_size = header.size;

            if event_size < header_size {
                debug!("reported event size is too small, no data here");
                return None;
            }

            if diff < event_size {
                debug!("gap between start and and is too small for described event");
                return None;
            }

            let event_start =
                (raw_header as *const _ as *const libc::c_void).offset(header_size as isize);

            let start = self.start;
            self.set_tail(start);
            self.prev = self.head();

            None
        }
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
}

// impl ::std::ops::Drop for RingBuffer {
//     fn drop(&mut self) {
//         unsafe {
//             libc::munmap(self.base, self.len);
//         }
//     }
// }

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
