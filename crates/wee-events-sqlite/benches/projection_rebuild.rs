//! Benchmark for `rebuild_projection` across a multi-partition strategy.
//!
//! Uses `AggregateStrategy` so every aggregate lives in its own database with
//! its own connection. That is the only configuration where rebuilding can
//! load aggregates concurrently — a single shared connection would serialise
//! every load on one mutex regardless of how the loop is written.
//!
//! Each measured iteration rebuilds into a *fresh* in-memory document store so
//! the "skip current documents" fast path never short-circuits the work.

use std::time::{Duration, Instant};

use criterion::{Criterion, criterion_group, criterion_main};
use serde::{Deserialize, Serialize};
use tokio::runtime::Runtime;
use wee_events::{
    AggregateId, AggregateType, EventData, EventStore as _, EventType, PublishOptions, RawEvent,
    ReduceFn, Renderer,
};
use wee_events_sqlite::{AggregateStrategy, DocumentStore, SqliteEventStore};

const AGGREGATE_COUNTS: &[usize] = &[50, 200, 800];
const EVENTS_PER_AGGREGATE: usize = 8;

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

/// Builds a local `AggregateStrategy` event store seeded with `count`
/// aggregates of `EVENTS_PER_AGGREGATE` events each. Returns the store, the
/// backing temp dir (kept alive by the caller), and the aggregate type.
async fn seed_event_store(
    count: usize,
) -> (
    SqliteEventStore<AggregateStrategy>,
    tempfile::TempDir,
    AggregateType,
) {
    let temp_dir = tempfile::tempdir().unwrap();
    let store = SqliteEventStore::open_local(temp_dir.path(), AggregateStrategy)
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

    (store, temp_dir, AggregateType::new("counter"))
}

fn bench_rebuild(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let renderer = counter_renderer();
    let mut group = c.benchmark_group("projection/rebuild/by_aggregate");

    for &count in AGGREGATE_COUNTS {
        let (store, _temp_dir, aggregate_type) = rt.block_on(seed_event_store(count));
        let store = &store;
        let aggregate_type = &aggregate_type;
        let renderer = &renderer;

        group.bench_function(format!("{count}"), |b| {
            b.to_async(&rt).iter_custom(move |iters| async move {
                let mut total = Duration::ZERO;
                for _ in 0..iters {
                    // Fresh document store each iteration so every aggregate is
                    // rebuilt from scratch (no skip-current short-circuit).
                    let document_store = DocumentStore::open_in_memory().await.unwrap();

                    let start = Instant::now();
                    wee_events_sqlite::rebuild_projection(
                        renderer,
                        store,
                        &document_store,
                        "counters",
                        aggregate_type,
                    )
                    .await
                    .expect("rebuild should succeed in bench");
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
