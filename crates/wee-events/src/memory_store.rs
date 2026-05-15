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
///
/// `MemoryStore` is `Clone`: cloning bumps an internal `Arc` and produces a
/// second handle pointing at the same underlying state. Use this when a test
/// needs to exercise multiple store instances backed by one logical
/// persistence layer.
#[derive(Clone)]
pub struct MemoryStore {
    backing: Arc<MemoryStoreBacking>,
}

/// Shared backing state behind a [`MemoryStore`]. Private — clone the store
/// itself to share its state with another handle.
struct MemoryStoreBacking {
    streams: Mutex<HashMap<AggregateId, Vec<Arc<RecordedEvent>>>>,
    generator: Mutex<Generator>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self {
            backing: Arc::new(MemoryStoreBacking::new()),
        }
    }

    /// Mints `count` paired `(EventId, Revision)` ULIDs in a single
    /// acquisition of the generator lock. Generator lock is released before
    /// the caller touches the streams lock — no lock-in-lock ordering.
    fn mint_event_ids(&self, count: usize) -> Result<Vec<(EventId, Revision)>, MemoryStoreError> {
        let mut generator = self
            .backing
            .generator
            .lock()
            .expect("ULID generator mutex poisoned");
        let mut out = Vec::with_capacity(count);
        for _ in 0..count {
            let event_id = generator
                .generate()
                .map_err(|e| MemoryStoreError::Ulid(Box::new(e)))?
                .to_string();
            let revision = generator
                .generate()
                .map_err(|e| MemoryStoreError::Ulid(Box::new(e)))?
                .to_string();
            out.push((EventId::new(event_id), Revision::new(revision)));
        }
        Ok(out)
    }

    /// Synchronous load helper. Factored out of [`EventStore::load`] so the
    /// `std::sync::MutexGuard` cannot cross an `.await` point — adding `.await`
    /// to the async wrapper does not risk the guard surviving across it,
    /// because the guard never escapes this function.
    fn load_sync(&self, id: &AggregateId) -> Aggregate {
        let streams = self.backing.streams.lock().expect("streams mutex poisoned");
        match streams.get(id) {
            Some(events) if !events.is_empty() => {
                // Vec<Arc<_>>::clone is N refcount bumps, not N deep copies.
                Aggregate::from_shared_events(id.clone(), events.clone())
            }
            _ => Aggregate::empty(id.clone()),
        }
    }

    /// Synchronous publish helper. Same guard-scoping rationale as
    /// [`Self::load_sync`]: streams lock never crosses an `.await`.
    /// Caller has already minted ULIDs via [`Self::mint_event_ids`].
    fn publish_sync(
        &self,
        aggregate_id: &AggregateId,
        options: PublishOptions,
        events: Vec<RawEvent>,
        minted: Vec<(EventId, Revision)>,
    ) -> Result<ChangeSet, MemoryStoreError> {
        let mut streams = self.backing.streams.lock().expect("streams mutex poisoned");
        let existing = streams.entry(aggregate_id.clone()).or_default();

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

        // Build each `RecordedEvent` once, wrap in `Arc`, push the same `Arc`
        // into both the store-side stream and the returned `ChangeSet`. One
        // allocation per event; the store retains and the caller observes via
        // refcount bumps.
        let recorded: Vec<Arc<RecordedEvent>> = events
            .into_iter()
            .zip(minted)
            .map(|(raw, (event_id, revision))| {
                Arc::new(RecordedEvent {
                    event_id,
                    event_type: raw.event_type,
                    revision,
                    metadata: metadata.clone(),
                    data: raw.data,
                })
            })
            .collect();

        let revision = recorded
            .last()
            .expect("recorded is non-empty: events.is_empty() was false")
            .revision
            .clone();
        existing.extend(recorded.iter().cloned());

        Ok(ChangeSet {
            aggregate_id: aggregate_id.clone(),
            revision,
            events: recorded,
        })
    }

    fn current_revision_sync(&self, id: &AggregateId) -> Revision {
        let streams = self.backing.streams.lock().expect("streams mutex poisoned");
        streams
            .get(id)
            .and_then(|v| v.last())
            .map(|e| e.revision.clone())
            .unwrap_or_else(Revision::zero)
    }
}

impl Default for MemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryStoreBacking {
    fn new() -> Self {
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
        Ok(self.load_sync(id))
    }

    async fn publish(
        &self,
        aggregate_id: &AggregateId,
        options: PublishOptions,
        events: Vec<RawEvent>,
    ) -> Result<ChangeSet, MemoryStoreError> {
        // Empty-events fast path: no IDs needed, single brief streams-lock acquisition.
        if events.is_empty() {
            return Ok(ChangeSet {
                aggregate_id: aggregate_id.clone(),
                revision: self.current_revision_sync(aggregate_id),
                events: Vec::new(),
            });
        }

        // Mint ULIDs first (generator lock only), then take the streams lock
        // with all IDs already in hand — no nested locks, no .await between
        // the two acquisitions.
        let minted = self.mint_event_ids(events.len())?;
        self.publish_sync(aggregate_id, options, events, minted)
    }
}

impl crate::EncodesEvents for MemoryStore {
    fn encoding(&self) -> crate::Encoding {
        crate::Encoding::Json
    }
}
