use futures_util::stream::{self, TryStreamExt};
use serde::Serialize;
use wee_events::{AggregateType, ChangeSet, EventStore as _, Renderer};

use crate::{DocumentStore, Error, PartitionCatalog, PartitionStrategy, SqliteEventStore};

/// Maximum number of aggregates loaded concurrently during a rebuild. Loads
/// fan out across per-partition connections; the document writes still
/// serialise on the document store's single connection.
const REBUILD_CONCURRENCY: usize = 16;

/// Applies a projection for a single aggregate after publish.
pub async fn apply_projection<S: Default + Serialize>(
    renderer: &Renderer<S>,
    event_store: &SqliteEventStore,
    document_store: &DocumentStore,
    changeset: &ChangeSet,
    collection: &str,
) -> Result<(), Error> {
    let aggregate = event_store.load(&changeset.aggregate_id).await?;
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
/// Aggregates are loaded concurrently (up to [`REBUILD_CONCURRENCY`]); under a
/// multi-partition strategy this fans loads out across independent connections.
pub async fn rebuild_projection<State, P, C>(
    renderer: &Renderer<State>,
    event_store: &SqliteEventStore<P, C>,
    document_store: &DocumentStore,
    collection: &str,
    aggregate_type: &AggregateType,
) -> Result<(), Error>
where
    State: Default + Serialize,
    P: PartitionStrategy,
    C: PartitionCatalog<P::Partition>,
{
    let aggregate_ids = event_store
        .enumerate_aggregates_by_type(aggregate_type)
        .await?;

    stream::iter(aggregate_ids.into_iter().map(Ok::<_, Error>))
        .try_for_each_concurrent(REBUILD_CONCURRENCY, |aggregate_id| async move {
            let aggregate = event_store.load(&aggregate_id).await?;

            if let Some(document) = document_store
                .get(collection, aggregate_id.aggregate_key())
                .await?
                && document.revision == *aggregate.revision()
            {
                return Ok(());
            }

            let entity = renderer.render(aggregate)?;
            let document = serde_json::to_value(&entity.state)?;

            document_store
                .upsert(
                    collection,
                    aggregate_id.aggregate_key(),
                    &entity.revision,
                    &document,
                )
                .await?;

            Ok(())
        })
        .await
}
