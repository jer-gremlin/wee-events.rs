//! Projection-rebuild benchmark modelling a remote (Turso/sqld) event store.
//!
//! The realistic case is one logical database per aggregate (or per user):
//! rebuilding loads N aggregates, each load a network round-trip. The win from
//! loading concurrently is therefore a function of round-trip latency, not of
//! local CPU — which is why a purely local bench shows nothing.
//!
//! This environment has no Docker, so we cannot stand up sqld. Instead we model
//! the remote backend: an in-memory event store seeded with N aggregates,
//! wrapped so every `load` first sleeps for a configurable latency `L`. The
//! number of physical databases is immaterial once `L` dominates the µs of a
//! local load — what matters is that there are N independent latency-bearing
//! loads, exactly as there would be against N remote databases. Absolute
//! numbers are synthetic; the sequential-vs-concurrent ratio is what transfers
//! to real Turso. Re-run against a real backend (set TURSO_*) for true numbers.
//!
//! Three variants per (N, L) cell:
//!   A `sequential`           — load + get + upsert, one aggregate at a time (the original loop)
//!   B `concurrent`           — loads fanned out 16-wide, still a `get`/`upsert` per row
//!   C `concurrent_batched`   — the shipped `rebuild_projection`: list once, fan out, one batched write
//! A→B isolates the load-concurrency win; B→C isolates the read-once + batched-write win.

use std::time::{Duration, Instant};

use criterion::{Criterion, criterion_group, criterion_main};
use futures_util::stream::{self, StreamExt, TryStreamExt};
use serde::{Deserialize, Serialize};
use tokio::runtime::Runtime;
use wee_events::{
    Aggregate, AggregateId, AggregateType, EventData, EventStore as _, EventType, PublishOptions,
    RawEvent, ReduceFn, Renderer,
};
use wee_events_sqlite::{
    DocumentStore, Error, GlobalStrategy, ProjectionSource, SqliteEventStore, rebuild_projection,
};

const EVENTS_PER_AGGREGATE: usize = 8;
const CONCURRENCY: usize = 16;

/// (aggregate count, modelled per-load latency). Cells where the sequential
/// variant would exceed ~1.5 s/iteration run only the concurrent variants.
const MATRIX: &[(usize, Duration)] = &[
    (200, Duration::ZERO),
    (1000, Duration::ZERO),
    (200, Duration::from_millis(1)),
    (1000, Duration::from_millis(1)),
    (200, Duration::from_millis(5)),
    (1000, Duration::from_millis(5)),
];

// ---------------------------------------------------------------------------
// Renderer
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct CounterState {
    value: i64,
    event_count: u32,
}

fn reduce_incremented(
    state: &mut CounterState,
    _event_type: &EventType,
    data: &EventData,
) -> Result<(), wee_events::DecodeError> {
    #[derive(Deserialize)]
    struct Payload {
        amount: i64,
    }
    let payload: Payload = data.deserialize_json()?;
    state.value += payload.amount;
    state.event_count += 1;
    Ok(())
}

fn counter_renderer() -> Renderer<CounterState> {
    Renderer::new().with(
        "counter:incremented",
        reduce_incremented as ReduceFn<CounterState>,
    )
}

fn make_event() -> RawEvent {
    RawEvent {
        event_type: EventType::new("counter:incremented"),
        data: EventData::json(&serde_json::json!({"amount": 1})).unwrap(),
    }
}

// ---------------------------------------------------------------------------
// Latency-injecting source — models a remote backend's per-load round-trip.
// ---------------------------------------------------------------------------

struct Delayed<'a, S> {
    inner: &'a S,
    latency: Duration,
}

impl<S: ProjectionSource + Sync> ProjectionSource for Delayed<'_, S> {
    async fn enumerate_aggregates_by_type(
        &self,
        aggregate_type: &AggregateType,
    ) -> Result<Vec<AggregateId>, Error> {
        self.inner
            .enumerate_aggregates_by_type(aggregate_type)
            .await
    }

    async fn load(&self, id: &AggregateId) -> Result<Aggregate, Error> {
        if !self.latency.is_zero() {
            tokio::time::sleep(self.latency).await;
        }
        self.inner.load(id).await
    }
}

// ---------------------------------------------------------------------------
// Variants A and B (C is the shipped `rebuild_projection`).
// ---------------------------------------------------------------------------

async fn rebuild_sequential<S: ProjectionSource + Sync>(
    renderer: &Renderer<CounterState>,
    src: &S,
    docs: &DocumentStore,
    collection: &str,
    aggregate_type: &AggregateType,
) -> Result<(), Error> {
    let ids = src.enumerate_aggregates_by_type(aggregate_type).await?;
    for id in ids {
        let aggregate = src.load(&id).await?;
        let key = id.aggregate_key().to_string();
        if let Some(doc) = docs.get(collection, &key).await?
            && doc.revision == *aggregate.revision()
        {
            continue;
        }
        let entity = renderer.render(aggregate)?;
        let value = serde_json::to_value(&entity.state)?;
        docs.upsert(collection, &key, &entity.revision, &value)
            .await?;
    }
    Ok(())
}

async fn rebuild_concurrent_unbatched<S: ProjectionSource + Sync>(
    renderer: &Renderer<CounterState>,
    src: &S,
    docs: &DocumentStore,
    collection: &str,
    aggregate_type: &AggregateType,
) -> Result<(), Error> {
    let ids = src.enumerate_aggregates_by_type(aggregate_type).await?;
    let renderer = &renderer;
    stream::iter(ids)
        .map(|id| async move {
            let aggregate = src.load(&id).await?;
            let key = id.aggregate_key().to_string();
            if let Some(doc) = docs.get(collection, &key).await?
                && doc.revision == *aggregate.revision()
            {
                return Ok::<_, Error>(());
            }
            let entity = renderer.render(aggregate)?;
            let value = serde_json::to_value(&entity.state)?;
            docs.upsert(collection, &key, &entity.revision, &value)
                .await?;
            Ok(())
        })
        .buffer_unordered(CONCURRENCY)
        .try_collect::<Vec<()>>()
        .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

async fn seed_store(count: usize) -> impl ProjectionSource + Sync {
    let store = SqliteEventStore::open_in_memory(GlobalStrategy)
        .await
        .unwrap();
    let events: Vec<RawEvent> = (0..EVENTS_PER_AGGREGATE).map(|_| make_event()).collect();
    for index in 0..count {
        let id = AggregateId::new("counter", format!("c{index}"));
        store
            .publish(&id, PublishOptions::default(), events.clone())
            .await
            .expect("seed publish should succeed");
    }
    store
}

fn bench_rebuild(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let renderer = counter_renderer();
    let renderer = &renderer;
    let aggregate_type = AggregateType::new("counter");
    let aggregate_type = &aggregate_type;

    let mut group = c.benchmark_group("projection/rebuild");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(8));
    group.warm_up_time(Duration::from_millis(500));

    for &(count, latency) in MATRIX {
        let store = rt.block_on(seed_store(count));
        let src = Delayed {
            inner: &store,
            latency,
        };
        let src = &src;
        let lat_ms = latency.as_millis();
        // Skip the O(N) sequential variant where it would dominate runtime.
        let run_sequential = count as u128 * lat_ms <= 1500;

        if run_sequential {
            group.bench_function(format!("sequential/n{count}/lat{lat_ms}ms"), |b| {
                b.to_async(&rt).iter_custom(|iters| async move {
                    let mut total = Duration::ZERO;
                    for _ in 0..iters {
                        let docs = DocumentStore::open_in_memory().await.unwrap();
                        let start = Instant::now();
                        rebuild_sequential(renderer, src, &docs, "counters", aggregate_type)
                            .await
                            .unwrap();
                        total += start.elapsed();
                    }
                    total
                });
            });
        }

        group.bench_function(format!("concurrent/n{count}/lat{lat_ms}ms"), |b| {
            b.to_async(&rt).iter_custom(|iters| async move {
                let mut total = Duration::ZERO;
                for _ in 0..iters {
                    let docs = DocumentStore::open_in_memory().await.unwrap();
                    let start = Instant::now();
                    rebuild_concurrent_unbatched(renderer, src, &docs, "counters", aggregate_type)
                        .await
                        .unwrap();
                    total += start.elapsed();
                }
                total
            });
        });

        group.bench_function(format!("concurrent_batched/n{count}/lat{lat_ms}ms"), |b| {
            b.to_async(&rt).iter_custom(|iters| async move {
                let mut total = Duration::ZERO;
                for _ in 0..iters {
                    let docs = DocumentStore::open_in_memory().await.unwrap();
                    let start = Instant::now();
                    rebuild_projection(renderer, src, &docs, "counters", aggregate_type)
                        .await
                        .unwrap();
                    total += start.elapsed();
                }
                total
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_rebuild);
criterion_main!(benches);
