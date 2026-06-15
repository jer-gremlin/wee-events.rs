use std::collections::HashMap;
use std::future::Future;

use futures_util::stream::{self, StreamExt, TryStreamExt};
use serde::Serialize;
use wee_events::{Aggregate, AggregateId, AggregateType, ChangeSet, Renderer, Revision};

use crate::{DocumentStore, Error, PartitionCatalog, PartitionStrategy, SqliteEventStore};

/// Maximum number of aggregates loaded concurrently during a rebuild. For
/// remote backends each load is a network round-trip, so the fan-out overlaps
/// latency; the batched write that follows commits in a single transaction.
const REBUILD_CONCURRENCY: usize = 16;

/// Read side of a projection rebuild: enumerate the aggregates of a type and
/// load each one. Implemented for [`SqliteEventStore`] regardless of partition
/// strategy; abstracting it lets a rebuild run over any event source.
pub trait ProjectionSource {
    fn enumerate_aggregates_by_type(
        &self,
        aggregate_type: &AggregateType,
    ) -> impl Future<Output = Result<Vec<AggregateId>, Error>> + Send;

    fn load(&self, id: &AggregateId) -> impl Future<Output = Result<Aggregate, Error>> + Send;
}

impl<P, C> ProjectionSource for SqliteEventStore<P, C>
where
    P: PartitionStrategy,
    C: PartitionCatalog<P::Partition>,
{
    async fn enumerate_aggregates_by_type(
        &self,
        aggregate_type: &AggregateType,
    ) -> Result<Vec<AggregateId>, Error> {
        SqliteEventStore::<P, C>::enumerate_aggregates_by_type(self, aggregate_type).await
    }

    async fn load(&self, id: &AggregateId) -> Result<Aggregate, Error> {
        wee_events::EventStore::load(self, id)
            .await
            .map_err(Into::into)
    }
}

/// Applies a projection for a single aggregate after publish.
pub async fn apply_projection<S: Default + Serialize>(
    renderer: &Renderer<S>,
    event_store: &SqliteEventStore,
    document_store: &DocumentStore,
    changeset: &ChangeSet,
    collection: &str,
) -> Result<(), Error> {
    let aggregate = wee_events::EventStore::load(event_store, &changeset.aggregate_id).await?;
    let entity = renderer.render(aggregate)?;
    let document = serde_json::to_value(&entity.state)?;

    document_store
        .upsert(
            collection,
            changeset.aggregate_id.aggregate_key(),
            &entity.revision,
            &document,
        )
        .await?;

    Ok(())
}

/// Rebuilds all projections for a given aggregate type.
///
/// Reads the collection's current revisions once, loads the aggregates
/// concurrently (up to [`REBUILD_CONCURRENCY`]), and writes the rendered
/// documents in a single batched transaction.
pub async fn rebuild_projection<State, S>(
    renderer: &Renderer<State>,
    event_store: &S,
    document_store: &DocumentStore,
    collection: &str,
    aggregate_type: &AggregateType,
) -> Result<(), Error>
where
    State: Default + Serialize,
    S: ProjectionSource + Sync,
{
    let aggregate_ids = event_store
        .enumerate_aggregates_by_type(aggregate_type)
        .await?;

    // One read for the whole collection instead of a `get` per aggregate.
    let existing: HashMap<String, Revision> = document_store
        .list(collection)
        .await?
        .into_iter()
        .map(|doc| (doc.key, doc.revision))
        .collect();
    let existing = &existing;
    let renderer = &renderer;

    let prepared: Vec<Option<(String, Revision, serde_json::Value)>> = stream::iter(aggregate_ids)
        .map(|aggregate_id| async move {
            let aggregate = event_store.load(&aggregate_id).await?;
            let key = aggregate_id.aggregate_key().to_string();

            if existing.get(&key) == Some(aggregate.revision()) {
                return Ok::<_, Error>(None);
            }

            let entity = renderer.render(aggregate)?;
            let document = serde_json::to_value(&entity.state)?;
            Ok(Some((key, entity.revision, document)))
        })
        .buffer_unordered(REBUILD_CONCURRENCY)
        .try_collect()
        .await?;

    let entries: Vec<(String, Revision, serde_json::Value)> =
        prepared.into_iter().flatten().collect();

    document_store.upsert_many(collection, &entries).await?;
    Ok(())
}
