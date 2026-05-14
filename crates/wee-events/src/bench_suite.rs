//! Event store performance benchmark suite.
//!
//! Provides reusable benchmark functions and a macro for running criterion
//! benchmarks against any `EventStore` implementation. Benchmarks are
//! organized into groups that isolate specific costs:
//!
//! - **creation** — first write to a new aggregate (includes partition provisioning)
//! - **steady_state** — writes to existing aggregates
//! - **load_scaling** — reads scaling by event count
//! - **partition_write** — concurrent writes spread across vs concentrated in partitions
//! - **partition_read** — concurrent reads spread across vs concentrated in partitions
//!
//! # Usage
//!
//! ```text
//! use criterion::criterion_main;
//! wee_events::testing::store_bench_suite!(my_store, MyStore::new());
//! criterion_main!(my_store::benches);
//! ```

use std::sync::Arc;
use std::time::{Duration, Instant};

use criterion::Criterion;
use tokio::runtime::Runtime;
use tokio::sync::Barrier;
use tokio::task::JoinSet;

use crate::id::AggregateId;
use crate::store::{EventStore, PublishOptions};
use crate::test_suite::make_raw_events;

/// Drives a `JoinSet` to completion, propagating panics. Replaces the prior
/// `join_all(...)` pattern which polled all futures on a single task — i.e.
/// not concurrent at all on a multi-thread runtime.
async fn drain<T: 'static>(mut set: JoinSet<T>) {
    while let Some(r) = set.join_next().await {
        r.expect("benchmark task panicked");
    }
}

/// Default concurrency levels for concurrent benchmarks.
pub const CONCURRENCY_LEVELS: &[usize] = &[2, 4, 8, 16, 32];

const LOAD_EVENT_COUNTS: &[usize] = &[1, 10, 50, 100, 500];
const BATCH_SIZES: &[usize] = &[1, 10, 50];

// ---------------------------------------------------------------------------
// Aggregate ID generators for partition-aware benchmarks
// ---------------------------------------------------------------------------

/// Creates an aggregate ID guaranteed to be in a unique partition for most
/// strategies. Each call with a different `index` produces a different
/// aggregate type, which TypeStrategy maps to a distinct partition. Other
/// strategies may or may not spread these across partitions.
fn make_spread_id(index: usize) -> AggregateId {
    AggregateId::new(format!("spread-{index}"), ulid::Ulid::new().to_string())
}

/// Creates an aggregate ID that shares a single aggregate type with all other
/// concentrated IDs. TypeStrategy maps these to the same partition.
fn make_concentrated_id() -> AggregateId {
    AggregateId::new("concentrated", ulid::Ulid::new().to_string())
}

/// Creates a generic test aggregate ID (same as conformance suite).
fn make_test_id() -> AggregateId {
    AggregateId::new("bench", ulid::Ulid::new().to_string())
}

/// Seeds an aggregate with `n` events. Returns the aggregate ID.
fn seed_aggregate<S: EventStore>(rt: &Runtime, store: &S, id: &AggregateId, event_count: usize) {
    if event_count == 0 {
        return;
    }
    let (_, raw) = make_raw_events(event_count);
    rt.block_on(store.publish(id, PublishOptions::default(), raw))
        .unwrap();
}

// ===========================================================================
// Group: creation — cost of first write (partition provisioning)
// ===========================================================================

/// Benchmark creating a single new aggregate (first publish).
/// This includes any partition/namespace provisioning cost.
pub fn bench_create_aggregate<S: EventStore>(
    c: &mut Criterion,
    rt: &Runtime,
    store: &S,
    prefix: &str,
) {
    c.bench_function(&format!("{prefix}/creation/single"), |b| {
        b.to_async(rt).iter(|| async {
            let id = make_test_id();
            let (_, raw) = make_raw_events(1);
            store
                .publish(&id, PublishOptions::default(), raw)
                .await
                .unwrap();
        });
    });
}

/// Benchmark creating N new aggregates concurrently, each in a different
/// partition (spread across partitions by varying aggregate type).
pub fn bench_create_spread<S: EventStore + 'static>(
    c: &mut Criterion,
    rt: &Runtime,
    store: &Arc<S>,
    prefix: &str,
    levels: &[usize],
) {
    let mut group = c.benchmark_group(format!("{prefix}/creation/spread"));
    for &n in levels {
        group.bench_function(format!("{n}"), |b| {
            b.to_async(rt).iter(|| {
                let store = Arc::clone(store);
                async move {
                    let mut set = JoinSet::new();
                    for i in 0..n {
                        let store = Arc::clone(&store);
                        set.spawn(async move {
                            let id = make_spread_id(i);
                            let (_, raw) = make_raw_events(1);
                            store
                                .publish(&id, PublishOptions::default(), raw)
                                .await
                                .expect("publish should succeed in bench");
                        });
                    }
                    drain(set).await;
                }
            });
        });
    }
    group.finish();
}

/// Benchmark creating N new aggregates concurrently, all in the same
/// partition (same aggregate type).
pub fn bench_create_concentrated<S: EventStore + 'static>(
    c: &mut Criterion,
    rt: &Runtime,
    store: &Arc<S>,
    prefix: &str,
    levels: &[usize],
) {
    let mut group = c.benchmark_group(format!("{prefix}/creation/concentrated"));
    for &n in levels {
        group.bench_function(format!("{n}"), |b| {
            b.to_async(rt).iter(|| {
                let store = Arc::clone(store);
                async move {
                    let mut set = JoinSet::new();
                    for _ in 0..n {
                        let store = Arc::clone(&store);
                        set.spawn(async move {
                            let id = make_concentrated_id();
                            let (_, raw) = make_raw_events(1);
                            store
                                .publish(&id, PublishOptions::default(), raw)
                                .await
                                .expect("publish should succeed in bench");
                        });
                    }
                    drain(set).await;
                }
            });
        });
    }
    group.finish();
}

// ===========================================================================
// Group: steady_state — writes to pre-existing aggregates
// ===========================================================================

/// Benchmark publishing varying batch sizes to an existing aggregate.
pub fn bench_publish_batch<S: EventStore>(
    c: &mut Criterion,
    rt: &Runtime,
    store: &S,
    prefix: &str,
) {
    let mut group = c.benchmark_group(format!("{prefix}/steady_state/publish_batch"));
    for &batch_size in BATCH_SIZES {
        group.bench_function(format!("{batch_size}"), |b| {
            // `iter_custom` so async setup is `.await`ed instead of
            // `rt.block_on`'d (which would deadlock — the whole closure
            // already runs inside Criterion's `rt.block_on`). Fresh
            // aggregate per measurement iteration kills the monotonic
            // growth that the old single-aggregate `iter` form had.
            b.to_async(rt).iter_custom(|iters| async move {
                let mut total = Duration::ZERO;
                for _ in 0..iters {
                    let id = make_test_id();
                    let (_, seed) = make_raw_events(1);
                    store
                        .publish(&id, PublishOptions::default(), seed)
                        .await
                        .expect("seed should succeed in bench");

                    let (_, raw) = make_raw_events(batch_size);
                    let start = Instant::now();
                    store
                        .publish(&id, PublishOptions::default(), raw)
                        .await
                        .expect("publish should succeed in bench");
                    total += start.elapsed();
                }
                total
            });
        });
    }
    group.finish();
}

/// Benchmark publish with optimistic concurrency check on an existing aggregate.
pub fn bench_publish_with_revision<S: EventStore>(
    c: &mut Criterion,
    rt: &Runtime,
    store: &S,
    prefix: &str,
) {
    c.bench_function(
        &format!("{prefix}/steady_state/publish_with_revision"),
        |b| {
            // `iter_custom` — see `bench_publish_batch` for the rationale.
            // Fresh aggregate per iter so the `load` step doesn't walk an
            // ever-longer event stream.
            b.to_async(rt).iter_custom(|iters| async move {
                let mut total = Duration::ZERO;
                for _ in 0..iters {
                    let id = make_test_id();
                    let (_, seed) = make_raw_events(1);
                    store
                        .publish(&id, PublishOptions::default(), seed)
                        .await
                        .expect("seed should succeed in bench");

                    let start = Instant::now();
                    let agg = store.load(&id).await.expect("load should succeed in bench");
                    let (_, raw) = make_raw_events(1);
                    let opts = PublishOptions {
                        expected_revision: Some(agg.revision().clone()),
                        ..Default::default()
                    };
                    store
                        .publish(&id, opts, raw)
                        .await
                        .expect("publish should succeed in bench");
                    total += start.elapsed();
                }
                total
            });
        },
    );
}

/// Benchmark appending to a growing aggregate stream.
pub fn bench_publish_append<S: EventStore>(
    c: &mut Criterion,
    rt: &Runtime,
    store: &S,
    prefix: &str,
) {
    c.bench_function(&format!("{prefix}/steady_state/append"), |b| {
        // `iter_custom` — see `bench_publish_batch` for the rationale.
        // Fresh aggregate per iter so the timed publish always lands on an
        // aggregate of length 1.
        b.to_async(rt).iter_custom(|iters| async move {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                let id = make_test_id();
                let (_, seed) = make_raw_events(1);
                store
                    .publish(&id, PublishOptions::default(), seed)
                    .await
                    .expect("seed should succeed in bench");

                let (_, raw) = make_raw_events(1);
                let start = Instant::now();
                store
                    .publish(&id, PublishOptions::default(), raw)
                    .await
                    .expect("publish should succeed in bench");
                total += start.elapsed();
            }
            total
        });
    });
}

// ===========================================================================
// Group: load_scaling — read performance by event count
// ===========================================================================

/// Benchmark loading aggregates with varying event counts.
pub fn bench_load_scaling<S: EventStore>(c: &mut Criterion, rt: &Runtime, store: &S, prefix: &str) {
    let mut group = c.benchmark_group(format!("{prefix}/load_scaling"));

    // Empty aggregate (non-existent)
    group.bench_function("0", |b| {
        b.to_async(rt).iter(|| async {
            let id = make_test_id();
            store.load(&id).await.unwrap();
        });
    });

    // Aggregates with events
    for &count in LOAD_EVENT_COUNTS {
        let id = make_test_id();
        seed_aggregate(rt, store, &id, count);

        group.bench_function(format!("{count}"), |b| {
            b.to_async(rt).iter(|| async {
                store.load(&id).await.unwrap();
            });
        });
    }
    group.finish();
}

// ===========================================================================
// Group: partition_write — concurrent writes spread vs concentrated
// ===========================================================================

/// Concurrent writes to pre-existing aggregates spread across different
/// partitions (each aggregate has a unique type).
pub fn bench_write_spread<S: EventStore + 'static>(
    c: &mut Criterion,
    rt: &Runtime,
    store: &Arc<S>,
    prefix: &str,
    levels: &[usize],
) {
    let mut group = c.benchmark_group(format!("{prefix}/partition_write/spread"));
    for &n in levels {
        group.bench_function(format!("{n}"), |b| {
            // `iter_custom` + `JoinSet`:
            //   setup (untimed) : build N fresh aggregates each seeded with 10 events
            //   timed region    : `tokio::spawn` one publish per aggregate, drain
            //
            // The previous form pre-seeded once outside the bench and let
            // every iter append to the SAME N aggregates — they grew to
            // ~10k events apiece — while collecting futures into a `Vec` and
            // `join_all`-ing them, which polls on one task (no parallelism).
            let store = Arc::clone(store);
            b.to_async(rt).iter_custom(move |iters| {
                let store = Arc::clone(&store);
                async move {
                    let mut total = Duration::ZERO;
                    for _ in 0..iters {
                        let ids: Vec<AggregateId> = (0..n).map(make_spread_id).collect();
                        for id in &ids {
                            let (_, seed) = make_raw_events(10);
                            store
                                .publish(id, PublishOptions::default(), seed)
                                .await
                                .expect("seed should succeed in bench");
                        }

                        let start = Instant::now();
                        let mut set = JoinSet::new();
                        for id in ids {
                            let store = Arc::clone(&store);
                            set.spawn(async move {
                                let (_, raw) = make_raw_events(1);
                                store
                                    .publish(&id, PublishOptions::default(), raw)
                                    .await
                                    .expect("publish should succeed in bench");
                            });
                        }
                        drain(set).await;
                        total += start.elapsed();
                    }
                    total
                }
            });
        });
    }
    group.finish();
}

/// Concurrent writes to pre-existing aggregates all within the same
/// partition (same aggregate type, different keys).
pub fn bench_write_concentrated<S: EventStore + 'static>(
    c: &mut Criterion,
    rt: &Runtime,
    store: &Arc<S>,
    prefix: &str,
    levels: &[usize],
) {
    let mut group = c.benchmark_group(format!("{prefix}/partition_write/concentrated"));
    for &n in levels {
        group.bench_function(format!("{n}"), |b| {
            // Same `iter_custom` + `JoinSet` shape as `bench_write_spread`,
            // except all aggregates share the "concentrated" type.
            let store = Arc::clone(store);
            b.to_async(rt).iter_custom(move |iters| {
                let store = Arc::clone(&store);
                async move {
                    let mut total = Duration::ZERO;
                    for _ in 0..iters {
                        let ids: Vec<AggregateId> =
                            (0..n).map(|_| make_concentrated_id()).collect();
                        for id in &ids {
                            let (_, seed) = make_raw_events(10);
                            store
                                .publish(id, PublishOptions::default(), seed)
                                .await
                                .expect("seed should succeed in bench");
                        }

                        let start = Instant::now();
                        let mut set = JoinSet::new();
                        for id in ids {
                            let store = Arc::clone(&store);
                            set.spawn(async move {
                                let (_, raw) = make_raw_events(1);
                                store
                                    .publish(&id, PublishOptions::default(), raw)
                                    .await
                                    .expect("publish should succeed in bench");
                            });
                        }
                        drain(set).await;
                        total += start.elapsed();
                    }
                    total
                }
            });
        });
    }
    group.finish();
}

/// Concurrent writes to the same aggregate (maximum contention).
pub fn bench_write_contention<S: EventStore + 'static>(
    c: &mut Criterion,
    rt: &Runtime,
    store: &Arc<S>,
    prefix: &str,
    levels: &[usize],
) {
    let mut group = c.benchmark_group(format!("{prefix}/partition_write/contention"));
    for &n in levels {
        group.bench_function(format!("{n}"), |b| {
            // `iter_custom` + Barrier-gated `JoinSet`:
            //   setup (untimed) : a fresh aggregate seeded with 1 event
            //   timed region    : N writers spawn, all `.wait()` on a
            //                     barrier, then publish simultaneously.
            //                     Real contention on one aggregate.
            //
            // The previous form pre-seeded once (aggregate grew across the
            // run) and used `join_all` which on a multi-thread runtime
            // polls every future on one task — no actual parallelism.
            // Errors were silently discarded via `let _ = …`.
            let store = Arc::clone(store);
            b.to_async(rt).iter_custom(move |iters| {
                let store = Arc::clone(&store);
                async move {
                    let mut total = Duration::ZERO;
                    for _ in 0..iters {
                        let id = make_test_id();
                        let (_, seed) = make_raw_events(1);
                        store
                            .publish(&id, PublishOptions::default(), seed)
                            .await
                            .expect("seed should succeed in bench");

                        let start = Instant::now();
                        let barrier = Arc::new(Barrier::new(n));
                        let mut set = JoinSet::new();
                        for _ in 0..n {
                            let store = Arc::clone(&store);
                            let id = id.clone();
                            let barrier = Arc::clone(&barrier);
                            set.spawn(async move {
                                barrier.wait().await;
                                let (_, raw) = make_raw_events(1);
                                // No `expected_revision`: every writer can
                                // commit; this measures pure publish-path
                                // contention on the store's locks.
                                store
                                    .publish(&id, PublishOptions::default(), raw)
                                    .await
                                    .expect("publish should succeed in bench");
                            });
                        }
                        drain(set).await;
                        total += start.elapsed();
                    }
                    total
                }
            });
        });
    }
    group.finish();
}

// ===========================================================================
// Group: partition_read — concurrent reads spread vs concentrated
// ===========================================================================

/// Concurrent reads from pre-existing aggregates spread across different
/// partitions.
pub fn bench_read_spread<S: EventStore + 'static>(
    c: &mut Criterion,
    rt: &Runtime,
    store: &Arc<S>,
    prefix: &str,
    levels: &[usize],
) {
    let mut group = c.benchmark_group(format!("{prefix}/partition_read/spread"));
    for &n in levels {
        let ids: Vec<_> = (0..n).map(make_spread_id).collect();
        for id in &ids {
            seed_aggregate(rt, &**store, id, 50);
        }

        group.bench_function(format!("{n}"), |b| {
            b.to_async(rt).iter(|| {
                let store = Arc::clone(store);
                let ids = ids.clone();
                async move {
                    let mut set = JoinSet::new();
                    for id in ids {
                        let store = Arc::clone(&store);
                        set.spawn(async move {
                            store.load(&id).await.expect("load should succeed in bench");
                        });
                    }
                    drain(set).await;
                }
            });
        });
    }
    group.finish();
}

/// Concurrent reads from pre-existing aggregates all within the same
/// partition.
pub fn bench_read_concentrated<S: EventStore + 'static>(
    c: &mut Criterion,
    rt: &Runtime,
    store: &Arc<S>,
    prefix: &str,
    levels: &[usize],
) {
    let mut group = c.benchmark_group(format!("{prefix}/partition_read/concentrated"));
    for &n in levels {
        let ids: Vec<_> = (0..n).map(|_| make_concentrated_id()).collect();
        for id in &ids {
            seed_aggregate(rt, &**store, id, 50);
        }

        group.bench_function(format!("{n}"), |b| {
            b.to_async(rt).iter(|| {
                let store = Arc::clone(store);
                let ids = ids.clone();
                async move {
                    let mut set = JoinSet::new();
                    for id in ids {
                        let store = Arc::clone(&store);
                        set.spawn(async move {
                            store.load(&id).await.expect("load should succeed in bench");
                        });
                    }
                    drain(set).await;
                }
            });
        });
    }
    group.finish();
}

// ===========================================================================
// Group: mixed — concurrent reads and writes across partitions
// ===========================================================================

/// Half the tasks read from spread partitions, half write to spread
/// partitions. Measures interference between readers and writers.
pub fn bench_mixed_read_write<S: EventStore + 'static>(
    c: &mut Criterion,
    rt: &Runtime,
    store: &Arc<S>,
    prefix: &str,
    levels: &[usize],
) {
    let mut group = c.benchmark_group(format!("{prefix}/mixed/read_write_spread"));
    for &n in levels {
        group.bench_function(format!("{n}r_{n}w"), |b| {
            // `iter_custom` + `JoinSet`:
            //   setup (untimed) : 2N fresh aggregates each seeded with 50 events.
            //                     Even indices are read targets, odd are write
            //                     targets — same convention as before.
            //   timed region    : spawn 2N tasks; readers `load`, writers
            //                     `publish`; drain.
            //
            // Previous form seeded once outside the bench and let writes
            // mutate that pool every iter — aggregates grew to enormous
            // size by the end of the run, slowing late iterations and
            // skewing the reported mean. With per-iter reseeding every
            // measurement starts at the same state.
            let store = Arc::clone(store);
            b.to_async(rt).iter_custom(move |iters| {
                let store = Arc::clone(&store);
                async move {
                    let mut total = Duration::ZERO;
                    for _ in 0..iters {
                        let all_ids: Vec<AggregateId> = (0..2 * n).map(make_spread_id).collect();
                        for id in &all_ids {
                            let (_, seed) = make_raw_events(50);
                            store
                                .publish(id, PublishOptions::default(), seed)
                                .await
                                .expect("seed should succeed in bench");
                        }

                        let start = Instant::now();
                        let mut set = JoinSet::new();
                        for (i, id) in all_ids.into_iter().enumerate() {
                            let store = Arc::clone(&store);
                            let is_reader = i % 2 == 0;
                            set.spawn(async move {
                                if is_reader {
                                    store.load(&id).await.expect("load should succeed in bench");
                                } else {
                                    let (_, raw) = make_raw_events(1);
                                    store
                                        .publish(&id, PublishOptions::default(), raw)
                                        .await
                                        .expect("publish should succeed in bench");
                                }
                            });
                        }
                        drain(set).await;
                        total += start.elapsed();
                    }
                    total
                }
            });
        });
    }
    group.finish();
}

// ===========================================================================
// Macro
// ===========================================================================

/// Generates a benchmark module that runs the full performance suite.
///
/// # Usage
///
/// ```text
/// // Default concurrency levels (2, 4, 8, 16, 32):
/// wee_events::testing::store_bench_suite!(memory_store, {
///     wee_events::memory::MemoryStore::new()
/// });
///
/// // Custom concurrency levels (for stores with expensive provisioning):
/// wee_events::testing::store_bench_suite!(heavy_store, &[2, 4, 8], {
///     HeavyStore::new().await
/// });
///
/// criterion_main!(memory_store::benches);
/// ```
#[macro_export]
macro_rules! store_bench_suite {
    ($mod_name:ident, $factory:expr) => {
        $crate::store_bench_suite!($mod_name, $crate::testing::CONCURRENCY_LEVELS, $factory);
    };
    ($mod_name:ident, $levels:expr, $factory:expr) => {
        mod $mod_name {
            use super::*;
            use criterion::{Criterion, criterion_group};
            use std::sync::Arc;

            fn store_benchmarks(c: &mut Criterion) {
                let rt = tokio::runtime::Runtime::new().unwrap();
                let store = rt.block_on(async { $factory });
                let store_arc = Arc::new(store);
                let store_ref = &*store_arc;
                let prefix = stringify!($mod_name);
                let levels: &[usize] = $levels;

                // Creation
                $crate::testing::bench_create_aggregate(c, &rt, store_ref, prefix);
                $crate::testing::bench_create_spread(c, &rt, &store_arc, prefix, levels);
                $crate::testing::bench_create_concentrated(c, &rt, &store_arc, prefix, levels);

                // Steady-state writes
                $crate::testing::bench_publish_batch(c, &rt, store_ref, prefix);
                $crate::testing::bench_publish_with_revision(c, &rt, store_ref, prefix);
                $crate::testing::bench_publish_append(c, &rt, store_ref, prefix);

                // Load scaling
                $crate::testing::bench_load_scaling(c, &rt, store_ref, prefix);

                // Partition write patterns
                $crate::testing::bench_write_spread(c, &rt, &store_arc, prefix, levels);
                $crate::testing::bench_write_concentrated(c, &rt, &store_arc, prefix, levels);
                $crate::testing::bench_write_contention(c, &rt, &store_arc, prefix, levels);

                // Partition read patterns
                $crate::testing::bench_read_spread(c, &rt, &store_arc, prefix, levels);
                $crate::testing::bench_read_concentrated(c, &rt, &store_arc, prefix, levels);

                // Mixed workload
                $crate::testing::bench_mixed_read_write(c, &rt, &store_arc, prefix, levels);
            }

            criterion_group!(benches, store_benchmarks);
        }
    };
}
