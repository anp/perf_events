pub mod config;
pub mod record;
pub mod ring_buffer;

use std::thread::{spawn, JoinHandle};

use futures::{self, prelude::*};

use self::{
    config::SamplingConfig,
    record::{Record, RecordDecoder},
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

    pub fn thread_id(&self) -> i32 {
        unimplemented!();
    }

    pub fn sampled<R>(self, f: impl FnOnce() -> R) -> Result<(R, Vec<Record>, Vec<Error>)> {
        let handle = self.start()?;
        let user_res = f();
        let samples = handle.join_with_remaining();
        Ok((user_res, samples.0, samples.1))
    }

    /// Launch the sampler on a separate thread, returning a handle from which sampled events can
    /// be collected.
    pub fn start(self) -> Result<SamplerHandle> {
        let Self { buffer: buf, .. } = self;

        buf.enable_fd()?;

        // three channels: a shutdown channel, a results channel, and an error channel
        let (stop, shutdown): (StopSender, StopReceiver) = ::futures::sync::oneshot::channel();
        let (record_sender, records): (RecordSender, RecordReceiver) = ::std::sync::mpsc::channel();
        let (error_sender, errors): (ErrorSender, ErrorReceiver) = ::std::sync::mpsc::channel();

        let sampler = spawn(move || {
            ::tokio::runtime::run(
                ::tokio_codec::FramedRead::new(
                    ::tokio::reactor::PollEvented2::new(buf),
                    RecordDecoder,
                ).map(move |r| {
                    record_sender
                        .send(r)
                        // if records are failing to send, we'll be cancelled soon anyways!
                        .unwrap_or_else(|_| ())
                })
                    // i'm of two minds about this -- if we fail to decode things, we might just
                    // to taint the entire sampler.
                    .map_err(move |e| error_sender.send(e).unwrap())
                    .select(shutdown.into_stream().map(|_| ()).map_err(|_| ()))
                    .for_each(|()| futures::future::ok(())),
            )
        });

        Ok(SamplerHandle {
            stop,
            records,
            errors,
            sampler,
        })
    }
}

struct StopSampling;
type StopSender = ::futures::sync::oneshot::Sender<StopSampling>;
type StopReceiver = ::futures::sync::oneshot::Receiver<StopSampling>;

pub type RecordSender = ::std::sync::mpsc::Sender<Record>;
pub type RecordReceiver = ::std::sync::mpsc::Receiver<Record>;

pub type ErrorSender = ::std::sync::mpsc::Sender<Error>;
pub type ErrorReceiver = ::std::sync::mpsc::Receiver<Error>;

pub struct SamplerHandle {
    stop: StopSender,
    records: RecordReceiver,
    errors: ErrorReceiver,
    sampler: JoinHandle<()>,
}

impl SamplerHandle {
    pub fn join_with_remaining(self) -> (Vec<Record>, Vec<Error>) {
        let _its_ok_if_we_already_sent_one = self.stop.send(StopSampling);
        self.sampler.join().unwrap();

        (
            self.records.into_iter().collect(),
            self.errors.into_iter().collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::config::*;
    use super::*;

    #[test]
    fn basic() {
        let _ = ::env_logger::Builder::from_default_env()
            .filter(None, ::log::LevelFilter::Info)
            .try_init();

        if let Err(why) = Sampler::new(SamplingConfig::default(), EventConfig::default()) {
            panic!("{}", why);
        }
    }
}
