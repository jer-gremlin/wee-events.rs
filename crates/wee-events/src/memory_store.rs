use std::sync::Arc;

use dashmap::DashMap;
use parking_lot::Mutex;
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
    streams: DashMap<AggregateId, Vec<Arc<RecordedEvent>>>,
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
    /// the caller touches the streams shard — no lock-in-lock ordering.
    fn mint_event_ids(&self, count: usize) -> Result<Vec<(EventId, Revision)>, MemoryStoreError> {
        let mut generator = self.backing.generator.lock();
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

    /// Synchronous load helper. The per-shard read guard from `DashMap`
    /// never crosses an `.await`.
    fn load_sync(&self, id: &AggregateId) -> Aggregate {
        match self.backing.streams.get(id) {
            Some(entry) if !entry.is_empty() => {
                Aggregate::from_shared_events(id.clone(), entry.clone())
            }
            _ => Aggregate::empty(id.clone()),
        }
    }

    /// Synchronous publish helper. Acquires only the per-shard write guard
    /// for `aggregate_id`; concurrent publishes to other aggregates land on
    /// different shards and don't contend.
    fn publish_sync(
        &self,
        aggregate_id: &AggregateId,
        options: PublishOptions,
        events: Vec<RawEvent>,
        minted: Vec<(EventId, Revision)>,
    ) -> Result<ChangeSet, MemoryStoreError> {
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

        let mut entry = self
            .backing
            .streams
            .entry(aggregate_id.clone())
            .or_default();

        if let Some(expected) = &options.expected_revision {
            let actual = entry
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

        entry.extend(recorded.iter().cloned());

        Ok(ChangeSet {
            aggregate_id: aggregate_id.clone(),
            revision,
            events: recorded,
        })
    }

    fn current_revision_sync(&self, id: &AggregateId) -> Revision {
        self.backing
            .streams
            .get(id)
            .and_then(|entry| entry.last().map(|e| e.revision.clone()))
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
            streams: DashMap::new(),
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
        self.backing
            .streams
            .iter()
            .map(|entry| entry.key().clone())
            .collect()
    }

    /// Returns all distinct aggregate IDs of a given type.
    pub fn enumerate_aggregates_by_type(&self, aggregate_type: &AggregateType) -> Vec<AggregateId> {
        self.backing
            .streams
            .iter()
            .filter(|entry| *entry.key().aggregate_type() == *aggregate_type)
            .map(|entry| entry.key().clone())
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
    #[inline]
    fn encoding(&self) -> crate::Encoding {
        crate::Encoding::Json
    }
}
