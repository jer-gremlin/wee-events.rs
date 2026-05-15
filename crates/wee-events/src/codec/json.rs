use serde::{Serialize, de::DeserializeOwned};

use super::{DecodeError, EncodeError, EventDecoder, EventEncoder};
use crate::event::EventData;

pub const ENCODING: &str = "application/json";

#[derive(Debug, Clone, Copy, Default)]
pub struct Encoder;

#[derive(Debug, Clone, Copy, Default)]
pub struct Decoder;

impl EventEncoder for Encoder {
    const ENCODING: &'static str = ENCODING;

    fn serialize<T>(&self, value: &T) -> Result<EventData, EncodeError>
    where
        T: Serialize,
    {
        Ok(EventData::raw(ENCODING, serde_json::to_vec(value)?))
    }
}

impl EventDecoder for Decoder {
    const ENCODING: &'static str = ENCODING;

    fn deserialize<T>(&self, data: &EventData) -> Result<T, DecodeError>
    where
        T: DeserializeOwned,
    {
        if data.encoding != ENCODING {
            return Err(DecodeError::EncodingMismatch {
                expected: ENCODING.to_string(),
                actual: data.encoding.to_string(),
            });
        }
        serde_json::from_slice(&data.data).map_err(DecodeError::Json)
    }
}
