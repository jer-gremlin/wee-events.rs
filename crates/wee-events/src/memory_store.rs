use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use ulid::Generator;

use crate::aggregate::Aggregate;
use crate::event::{ChangeSet, EventMetadata, RecordedEvent};
use crate::id::{AggregateId, AggregateType, EventId, Revision};
use crate::store::{EventStore, PublishOptions, RawEvent};

/// Errors produced by [`MemoryStore`].
///
/// The `WeeEvents` variant carries structural failures from `crate::Error`
/// (revision conflicts, encoding mismatches, retry exhaustion), satisfying
/// the `From<crate::Error>` bound required by the [`EventStore`] trait.
#[derive(Debug, thiserror::Error)]
pub enum MemoryStoreError {
    #[error(transparent)]
    WeeEvents(#[from] crate::Error),
    #[error("ulid generation: {0}")]
    Ulid(Box<dyn std::error::Error + Send + Sync>),
    #[error("codec: {0}")]
    Codec(#[from] crate::EncodeError),
}

impl crate::EventStoreErrorExt for MemoryStoreError {
    fn as_wee_events(&self) -> Option<&crate::Error> {
        match self {
            MemoryStoreError::WeeEvents(e) => Some(e),
            _ => None,
        }
    }
}

impl From<serde_json::Error> for MemoryStoreError {
    fn from(error: serde_json::Error) -> Self {
        Self::Codec(crate::EncodeError::Json(error))
    }
}

/// In-memory event store for testing. Thread-safe via `Mutex`.
///
/// Uses a monotonic ULID generator — guarantees strictly increasing
/// revisions even within the same millisecond.
pub struct MemoryStore {
    backing: Arc<MemoryStoreBacking>,
}

/// Shared backing state for one or more [`MemoryStore`] handles.
///
/// Multiple stores created with the same backing observe the same aggregate
/// streams and revision generator, which is useful for contract tests that
/// exercise multiple store instances over one logical persistence layer.
pub struct MemoryStoreBacking {
    streams: Mutex<HashMap<AggregateId, Vec<RecordedEvent>>>,
    generator: Mutex<Generator>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::from_shared(Arc::new(MemoryStoreBacking::new()))
    }

    pub fn from_shared(backing: Arc<MemoryStoreBacking>) -> Self {
        Self { backing }
    }

    fn generate_ulid(&self) -> Result<String, MemoryStoreError> {
        self.backing
            .generator
            .lock()
            .expect("ULID generator mutex poisoned")
            .generate()
            .map(|u| u.to_string())
            .map_err(|e| MemoryStoreError::Ulid(Box::new(e)))
    }
}

impl Default for MemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryStoreBacking {
    pub fn new() -> Self {
        Self {
            streams: Mutex::new(HashMap::new()),
            generator: Mutex::new(Generator::new()),
        }
    }
}

impl Default for MemoryStoreBacking {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryStore {
    /// Returns all distinct aggregate IDs in the store.
    pub fn enumerate_aggregates(&self) -> Vec<AggregateId> {
        let streams = self.backing.streams.lock().expect("streams mutex poisoned");
        streams.keys().cloned().collect()
    }

    /// Returns all distinct aggregate IDs of a given type.
    pub fn enumerate_aggregates_by_type(&self, aggregate_type: &AggregateType) -> Vec<AggregateId> {
        let streams = self.backing.streams.lock().expect("streams mutex poisoned");
        streams
            .keys()
            .filter(|id| *id.aggregate_type() == *aggregate_type)
            .cloned()
            .collect()
    }
}

impl EventStore for MemoryStore {
    type Error = MemoryStoreError;

    async fn load(&self, id: &AggregateId) -> Result<Aggregate, MemoryStoreError> {
        let streams = self.backing.streams.lock().expect("streams mutex poisoned");

        match streams.get(id) {
            Some(events) if !events.is_empty() => {
                Ok(Aggregate::from_events(id.clone(), events.clone()))
            }
            _ => Ok(Aggregate::empty(id.clone())),
        }
    }

    async fn publish(
        &self,
        aggregate_id: &AggregateId,
        options: PublishOptions,
        events: Vec<RawEvent>,
    ) -> Result<ChangeSet, MemoryStoreError> {
        let mut streams = self.backing.streams.lock().expect("streams mutex poisoned");

        if events.is_empty() {
            let revision = streams
                .get(aggregate_id)
                .and_then(|v| v.last())
                .map(|e| e.revision.clone())
                .unwrap_or_else(Revision::zero);
            return Ok(ChangeSet {
                aggregate_id: aggregate_id.clone(),
                revision,
                events: Vec::new(),
            });
        }

        let existing = streams.entry(aggregate_id.clone()).or_default();

        // Optimistic concurrency check
        if let Some(expected) = &options.expected_revision {
            let actual = existing
                .last()
                .map(|e| e.revision.clone())
                .unwrap_or_else(Revision::zero);
            if *expected != actual {
                return Err(MemoryStoreError::WeeEvents(
                    crate::Error::RevisionConflict {
                        expected: expected.clone(),
                        actual,
                    },
                ));
            }
        }

        let metadata = EventMetadata {
            causation_id: options.causation_id,
            correlation_id: options.correlation_id,
        };

        let mut recorded = Vec::with_capacity(events.len());
        for raw in events {
            let event = RecordedEvent {
                event_id: EventId::new(self.generate_ulid()?),
                event_type: raw.event_type,
                revision: Revision::new(self.generate_ulid()?),
                metadata: metadata.clone(),
                data: raw.data,
            };
            recorded.push(event);
        }

        let revision = recorded
            .last()
            .expect("recorded is non-empty: events.is_empty() was false")
            .revision
            .clone();
        existing.extend(recorded.clone());

        Ok(ChangeSet {
            aggregate_id: aggregate_id.clone(),
            revision,
            events: recorded,
        })
    }
}

static JSON_ENCODER: crate::JsonEncoder = crate::JsonEncoder;

impl crate::EncodesEvents for MemoryStore {
    type Encoder = crate::JsonEncoder;

    fn event_encoder(&self) -> &Self::Encoder {
        &JSON_ENCODER
    }
}
