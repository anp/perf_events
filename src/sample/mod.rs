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

/// Launch the sampler on a separate thread, returning a handle from which sampled events can
/// be collected.
pub fn sampler(sample_config: SamplingConfig) -> Result<SamplerHandle> {
    debug!("enabling our ring buffer's file descriptor");

    // three channels: a shutdown channel, a results channel, and an error channel
    let (stop, shutdown): (StopSender, StopReceiver) = ::futures::sync::oneshot::channel();
    let (record_sender, records) = channel::unbounded();
    let (error_sender, error) = channel::bounded(1);

    let outer_error_sender = error_sender.clone();

    debug!("spawning sampler thread");
    let sampler = spawn(move || {
        let f = move || -> Result<()> {
            use futures::{
                future::{empty, ok},
                Stream,
            };
            use tokio::runtime::current_thread::Runtime;

            debug!("creating executor");
            let mut rt = Runtime::new()?;
            rt.spawn(empty()); // start the runtime

            let buffer = RingBuffer::new(sample_config)?;
            buffer.enable_fd()?;

            // we want to keep running the sampler in the background on this thread
            debug!("spawning decoder");
            rt.spawn(Decoder::new(buffer, record_sender, error_sender).for_each(|()| ok(())));

            // this runs the executor until the shutdown channel has a value
            debug!("running executor until shutdown message received");
            rt.block_on(shutdown).unwrap();

            debug!("shutdown message received, sampler thread exiting");
            Ok(())
        };

        if let Err(why) = f() {
            error!("error on sampler thread: {:?}", why);
            outer_error_sender.send(why);
        }
    });

    Ok(SamplerHandle {
        stop,
        records,
        error,
        sampler,
    })
}

pub fn sampled<R>(
    sample_config: SamplingConfig,
    f: impl FnOnce() -> R,
) -> ::std::result::Result<(R, Vec<Record>), (Option<R>, Error)> {
    info!("starting sampler");
    let handle = match sampler(sample_config) {
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

    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    #[test]
    fn basic() {
        let _ = ::env_logger::Builder::from_default_env()
            .filter(None, ::log::LevelFilter::Debug)
            .filter(Some("tokio_reactor"), ::log::LevelFilter::Info)
            .try_init();

        let mut res = Vec::new();

        let ((), samples) = sampled(SamplingConfig::default(), || {
            info!("starting fake bench run");
            for _ in 0..1_000 {
                let mut hasher = DefaultHasher::new();
                (0..5_000)
                    .into_iter()
                    .map(|n| (::rand::random(), n))
                    .collect::<Vec<(u64, usize)>>()
                    .hash(&mut hasher);
                res.push(hasher.finish());
            }
            info!("fake bench run complete");
        }).unwrap();
        assert_ne!(samples.len(), 0);
    }
}
