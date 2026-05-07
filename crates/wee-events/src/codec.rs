use serde::{de::DeserializeOwned, Serialize};

use crate::event::EventData;

pub trait EventEncoder {
    const ENCODING: &'static str;

    fn serialize<T>(&self, value: &T) -> Result<EventData, EncodeError>
    where
        T: Serialize;
}

pub trait EventDecoder {
    const ENCODING: &'static str;

    fn deserialize<T>(&self, data: &EventData) -> Result<T, DecodeError>
    where
        T: DeserializeOwned;
}

pub trait EncodesEvents {
    type Encoder: EventEncoder;

    fn event_encoder(&self) -> &Self::Encoder;
}

impl<T> EncodesEvents for std::sync::Arc<T>
where
    T: EncodesEvents + ?Sized,
{
    type Encoder = T::Encoder;

    fn event_encoder(&self) -> &Self::Encoder {
        (**self).event_encoder()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EncodeError {
    #[error("json encode: {0}")]
    Json(#[from] serde_json::Error),
    #[error("cbor encode: {0}")]
    Cbor(#[from] ciborium::ser::Error<std::io::Error>),
}

#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("unknown encoding: {encoding}")]
    UnknownEncoding { encoding: String },
    #[error("encoding mismatch: expected {expected}, actual {actual}")]
    EncodingMismatch { expected: String, actual: String },
    #[error("json decode: {0}")]
    Json(#[from] serde_json::Error),
    #[error("cbor decode: {0}")]
    Cbor(#[from] ciborium::de::Error<std::io::Error>),
}

/// Unified codec failure covering both encode and decode directions.
///
/// Used at boundaries where a single error type needs to carry either kind
/// of codec failure (e.g. `ServiceError::Codec`).
#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    #[error(transparent)]
    Encode(#[from] EncodeError),
    #[error(transparent)]
    Decode(#[from] DecodeError),
}

#[derive(Debug, Clone, Copy, Default)]
pub struct JsonEncoder;

#[derive(Debug, Clone, Copy, Default)]
pub struct JsonDecoder;

#[derive(Debug, Clone, Copy, Default)]
pub struct CborEncoder;

#[derive(Debug, Clone, Copy, Default)]
pub struct CborDecoder;

impl EventEncoder for JsonEncoder {
    const ENCODING: &'static str = EventData::JSON_ENCODING;

    fn serialize<T>(&self, value: &T) -> Result<EventData, EncodeError>
    where
        T: Serialize,
    {
        Ok(EventData::raw(Self::ENCODING, serde_json::to_vec(value)?))
    }
}

impl EventDecoder for JsonDecoder {
    const ENCODING: &'static str = EventData::JSON_ENCODING;

    fn deserialize<T>(&self, data: &EventData) -> Result<T, DecodeError>
    where
        T: DeserializeOwned,
    {
        if data.encoding != Self::ENCODING {
            return Err(DecodeError::EncodingMismatch {
                expected: Self::ENCODING.to_string(),
                actual: data.encoding.clone(),
            });
        }

        serde_json::from_slice(&data.data).map_err(DecodeError::Json)
    }
}

impl EventEncoder for CborEncoder {
    const ENCODING: &'static str = "application/cbor";

    fn serialize<T>(&self, value: &T) -> Result<EventData, EncodeError>
    where
        T: Serialize,
    {
        let mut data = Vec::new();
        ciborium::into_writer(value, &mut data)?;
        Ok(EventData::raw(Self::ENCODING, data))
    }
}

impl EventDecoder for CborDecoder {
    const ENCODING: &'static str = CborEncoder::ENCODING;

    fn deserialize<T>(&self, data: &EventData) -> Result<T, DecodeError>
    where
        T: DeserializeOwned,
    {
        if data.encoding != Self::ENCODING {
            return Err(DecodeError::EncodingMismatch {
                expected: Self::ENCODING.to_string(),
                actual: data.encoding.clone(),
            });
        }

        ciborium::from_reader(data.data.as_slice()).map_err(DecodeError::Cbor)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Nil;

#[derive(Debug, Clone, Copy)]
pub struct Cons<Head, Tail> {
    head: Head,
    tail: Tail,
}

#[derive(Debug, Clone, Copy)]
pub struct EventDecoders<List = Nil> {
    list: List,
}

impl EventDecoders<Nil> {
    pub fn new() -> Self {
        Self { list: Nil }
    }
}

impl Default for EventDecoders<Nil> {
    fn default() -> Self {
        Self::new()
    }
}

impl<List> EventDecoders<List> {
    pub fn with<D>(self, decoder: D) -> EventDecoders<Cons<D, List>>
    where
        D: EventDecoder,
    {
        EventDecoders {
            list: Cons {
                head: decoder,
                tail: self.list,
            },
        }
    }
}

impl<List> EventDecoders<List>
where
    List: DecoderList,
{
    pub fn deserialize<T>(&self, data: &EventData) -> Result<T, DecodeError>
    where
        T: DeserializeOwned,
    {
        self.list.deserialize(data)
    }
}

pub trait DecoderList {
    fn deserialize<T>(&self, data: &EventData) -> Result<T, DecodeError>
    where
        T: DeserializeOwned;
}

impl DecoderList for Nil {
    fn deserialize<T>(&self, data: &EventData) -> Result<T, DecodeError>
    where
        T: DeserializeOwned,
    {
        Err(DecodeError::UnknownEncoding {
            encoding: data.encoding.clone(),
        })
    }
}

impl<Head, Tail> DecoderList for Cons<Head, Tail>
where
    Head: EventDecoder,
    Tail: DecoderList,
{
    fn deserialize<T>(&self, data: &EventData) -> Result<T, DecodeError>
    where
        T: DeserializeOwned,
    {
        if data.encoding == Head::ENCODING {
            self.head.deserialize(data)
        } else {
            self.tail.deserialize(data)
        }
    }
}
