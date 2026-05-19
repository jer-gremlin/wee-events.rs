use serde::{Deserialize, Serialize};

use crate::codec::Encoding;
use crate::id::{AggregateId, CorrelationId, EventId, EventType, Revision};

/// Encoding-tagged payload. The store treats this as opaque bytes with an
/// encoding discriminator — it never interprets the content.
///
/// `encoding` is the typed [`Encoding`] enum — the type system forbids
/// constructing an `EventData` with an unknown encoding. Stores reading from
/// foreign data sources (sqlite TEXT columns, NATS messages) must convert
/// the wire string via [`Encoding::from_encoding_str`] and propagate the
/// `UnknownEncoding` error at row-read time.
#[derive(Debug, Clone)]
pub struct EventData {
    pub encoding: Encoding,
    pub data: Vec<u8>,
}

/// Error returned by [`EventData::deserialize_json`].
///
/// The two variants distinguish a structural failure (the payload is not in
/// the expected encoding) from a codec failure (the bytes are JSON but did not
/// deserialize into the target type). Callers can preserve that distinction
/// when mapping into a service-level error rather than collapsing both into
/// a single stringly-typed message.
#[derive(Debug, thiserror::Error)]
pub enum DeserializeJsonError {
    #[error("encoding mismatch: expected {expected:?}, actual {actual:?}")]
    EncodingMismatch {
        expected: Encoding,
        actual: Encoding,
    },
    #[error(transparent)]
    Decode(#[from] serde_json::Error),
}

impl DeserializeJsonError {
    /// Project this error into any store error type. `EncodingMismatch`
    /// becomes a structural [`crate::Error::EncodingMismatch`]; `Decode`
    /// becomes a raw [`serde_json::Error`]. Useful for [`crate::Renderer`]
    /// callers whose return type is the store's associated `Error`.
    pub fn into_store_error<E>(self) -> E
    where
        E: From<crate::Error> + From<serde_json::Error>,
    {
        match self {
            Self::EncodingMismatch { expected, actual } => crate::Error::EncodingMismatch {
                expected: expected.as_str().to_string(),
                actual: actual.as_str().to_string(),
            }
            .into(),
            Self::Decode(e) => e.into(),
        }
    }
}

impl EventData {
    /// Creates an `EventData` from a JSON-serializable value.
    #[inline]
    pub fn json<T: Serialize>(value: &T) -> Result<Self, crate::EncodeError> {
        Ok(Self {
            encoding: Encoding::Json,
            data: serde_json::to_vec(value)?,
        })
    }

    /// Creates an `EventData` from raw bytes with a given encoding.
    pub fn raw(encoding: Encoding, data: Vec<u8>) -> Self {
        Self { encoding, data }
    }

    /// Returns true if this payload is JSON-encoded.
    #[inline]
    pub fn is_json(&self) -> bool {
        matches!(self.encoding, Encoding::Json)
    }

    /// Deserializes the payload as JSON into the target type.
    ///
    /// Returns `DeserializeJsonError::EncodingMismatch` if the payload is not
    /// JSON-encoded, or `DeserializeJsonError::Decode` if JSON parsing fails.
    pub fn deserialize_json<T: for<'de> Deserialize<'de>>(
        &self,
    ) -> Result<T, DeserializeJsonError> {
        if !self.is_json() {
            return Err(DeserializeJsonError::EncodingMismatch {
                expected: Encoding::Json,
                actual: self.encoding,
            });
        }
        Encoding::Json.decode(self).map_err(|err| match err {
            crate::DecodeError::EncodingMismatch { expected, actual } => {
                DeserializeJsonError::EncodingMismatch { expected, actual }
            }
            crate::DecodeError::Json(e) => DeserializeJsonError::Decode(e),
            other => DeserializeJsonError::Decode(serde_json::Error::io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                other,
            ))),
        })
    }
}

/// A recorded event at the store boundary. Domain-agnostic — the store never
/// sees concrete domain event types, only `EventType` + `EventData`.
///
/// Not `Serialize`/`Deserialize` — stores decompose into columns/fields.
/// Shared stores (NATS, DynamoDB) will add Go-compatible JSON serialization.
#[derive(Debug, Clone)]
pub struct RecordedEvent {
    pub event_id: EventId,
    pub event_type: EventType,
    pub revision: Revision,
    pub metadata: EventMetadata,
    pub data: EventData,
}

/// Metadata attached to every recorded event for tracing and correlation.
#[derive(Debug, Clone, Default)]
pub struct EventMetadata {
    pub causation_id: Option<EventId>,
    pub correlation_id: Option<CorrelationId>,
}

/// An atomic batch of events from a single publish call. Events within a
/// changeset are applied together — all or nothing.
///
/// Events share storage with the store-side stream via `Arc` — both the
/// `Aggregate` retained by the store and this `ChangeSet` returned to the
/// caller point at the same allocations.
#[derive(Debug, Clone)]
pub struct ChangeSet {
    pub aggregate_id: AggregateId,
    pub revision: Revision,
    pub events: Vec<std::sync::Arc<RecordedEvent>>,
}

/// Trait implemented by domain event enums. The derive macro generates this
/// from the enum variant names (kebab-case, prefixed).
///
/// Domain events must be serializable and deserializable so they can round-trip
/// across the store boundary.
pub trait DomainEvent: Serialize + for<'de> Deserialize<'de> + Send + 'static {
    /// Returns the stable event type discriminator for this variant.
    fn event_type(&self) -> EventType;
}
