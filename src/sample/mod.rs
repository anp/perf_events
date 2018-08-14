pub mod config;
pub mod record;
pub mod ring_buffer;

use std::thread::{spawn, JoinHandle};

use self::{
    config::SamplingConfig,
    record::{Decoder, Record},
    ring_buffer::RingBuffer,
};
use super::EventConfig;
use error::*;

pub struct Sampler {
    _config: SamplingConfig,
    buffer: RingBuffer,
}

impl Sampler {
    pub fn new(sample_config: SamplingConfig, event_config: EventConfig) -> Result<Self> {
        Ok(Self {
            _config: sample_config.clone(),
            buffer: RingBuffer::new(sample_config, event_config)?,
        })
    }

    pub fn sampled<R>(
        self,
        f: impl FnOnce() -> R,
    ) -> ::std::result::Result<(R, Vec<Record>), (Option<R>, Error)> {
        info!("starting sampler");
        let handle = match self.start() {
            Ok(h) => h,
            Err(why) => return Err((None, why)),
        };

        let user_res = f();

        info!("terminating sampler");
        match handle.join_with_remaining() {
            Ok(samples) => Ok((user_res, samples)),
            Err(why) => Err((Some(user_res), why)),
        }
    }

    /// Launch the sampler on a separate thread, returning a handle from which sampled events can
    /// be collected.
    pub fn start(self) -> Result<SamplerHandle> {
        let Self { buffer: buf, .. } = self;

        debug!("enabling our ring buffer's file descriptor");
        buf.enable_fd()?;

        // three channels: a shutdown channel, a results channel, and an error channel
        let (stop, shutdown): (StopSender, StopReceiver) = ::futures::sync::oneshot::channel();
        let (record_sender, records) = channel::unbounded();
        let (error_sender, error) = channel::bounded(1);

        debug!("spawning sampler thread");
        let sampler = spawn(move || {
            use futures::{future::ok, Stream};
            use tokio::executor::current_thread::CurrentThread;

            debug!("creating executor");
            let mut executor = CurrentThread::new();

            // we want to keep running the sampler in the background on this thread
            debug!("spawning decoder");
            executor.spawn(Decoder::new(buf, record_sender, error_sender).for_each(|()| ok(())));

            // this runs the executor until the shutdown channel has a value
            debug!("running executor until shutdown message received");
            executor.block_on(shutdown).unwrap();

            debug!("shutdown message received, sampler thread exiting");
        });

        Ok(SamplerHandle {
            stop,
            records,
            error,
            sampler,
        })
    }
}

use channel::{self, Receiver};

pub struct StopSampling;
type StopSender = ::futures::sync::oneshot::Sender<StopSampling>;
pub(crate) type StopReceiver = ::futures::sync::oneshot::Receiver<StopSampling>;

pub struct SamplerHandle {
    stop: StopSender,
    records: Receiver<Record>,
    error: Receiver<Error>,
    sampler: JoinHandle<()>,
}

impl SamplerHandle {
    pub fn join_with_remaining(self) -> Result<Vec<Record>> {
        debug!("sending stop signal to sampler thread");
        let _its_ok_if_we_already_sent_one = self.stop.send(StopSampling);

        debug!("joining on sampler thread");
        if let Err(why) = self.sampler.join() {
            return Err(Error::Misc {
                inner: ::failure::err_msg(format!("sampler thread panicked: {:?}", why)),
            });
        }

        debug!("sampler thread has terminated");
        if let Some(e) = self.error.recv() {
            return Err(e);
        }

        debug!("no errors reported from sampler thread.");

        Ok(self.records.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::config::*;
    use super::*;

    #[test]
    fn basic() {
        let _ = ::env_logger::Builder::from_default_env()
            .filter(None, ::log::LevelFilter::Debug)
            .try_init();

        let sampler = Sampler::new(SamplingConfig::default(), EventConfig::default()).unwrap();

        let ((), samples) = sampler
            .sampled(|| {
                info!("starting fake bench run");
                for _ in 0..10_000_000 {
                    // do something
                    trace!("noop");
                }
                info!("fake bench run complete");
            })
            .unwrap();
    }
}
