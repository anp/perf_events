use tokio::prelude::*;
use tokio_codec::Decoder;

pub struct RecordDecoder;

impl Decoder for RecordDecoder {}
