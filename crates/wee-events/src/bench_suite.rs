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

use criterion::Criterion;
use futures::future::join_all;
use tokio::runtime::Runtime;

use crate::id::AggregateId;
use crate::store::{EventStore, PublishOptions};
use crate::test_suite::make_raw_events;

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
        // Use a fresh batch of spread IDs per iteration via iter_batched.
        group.bench_function(format!("{n}"), |b| {
            b.to_async(rt).iter(|| {
                let store = Arc::clone(store);
                async move {
                    let futs: Vec<_> = (0..n)
                        .map(|i| {
                            let store = Arc::clone(&store);
                            async move {
                                let id = make_spread_id(i);
                                let (_, raw) = make_raw_events(1);
                                store
                                    .publish(&id, PublishOptions::default(), raw)
                                    .await
                                    .unwrap();
                            }
                        })
                        .collect();
                    join_all(futs).await;
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
                    let futs: Vec<_> = (0..n)
                        .map(|_| {
                            let store = Arc::clone(&store);
                            async move {
                                let id = make_concentrated_id();
                                let (_, raw) = make_raw_events(1);
                                store
                                    .publish(&id, PublishOptions::default(), raw)
                                    .await
                                    .unwrap();
                            }
                        })
                        .collect();
                    join_all(futs).await;
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
        let id = make_test_id();
        seed_aggregate(rt, store, &id, 1);

        group.bench_function(format!("{batch_size}"), |b| {
            b.to_async(rt).iter(|| async {
                let (_, raw) = make_raw_events(batch_size);
                store
                    .publish(&id, PublishOptions::default(), raw)
                    .await
                    .unwrap();
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
    let id = make_test_id();
    seed_aggregate(rt, store, &id, 1);

    c.bench_function(
        &format!("{prefix}/steady_state/publish_with_revision"),
        |b| {
            b.to_async(rt).iter(|| async {
                let agg = store.load(&id).await.unwrap();
                let (_, raw) = make_raw_events(1);
                let opts = PublishOptions {
                    expected_revision: Some(agg.revision().clone()),
                    ..Default::default()
                };
                store.publish(&id, opts, raw).await.unwrap();
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
    let id = make_test_id();
    seed_aggregate(rt, store, &id, 1);

    c.bench_function(&format!("{prefix}/steady_state/append"), |b| {
        b.to_async(rt).iter(|| async {
            let (_, raw) = make_raw_events(1);
            store
                .publish(&id, PublishOptions::default(), raw)
                .await
                .unwrap();
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
        // Pre-provision: each aggregate in its own partition.
        let ids: Vec<_> = (0..n).map(make_spread_id).collect();
        for id in &ids {
            seed_aggregate(rt, &**store, id, 10);
        }

        group.bench_function(format!("{n}"), |b| {
            b.to_async(rt).iter(|| {
                let store = Arc::clone(store);
                let ids = ids.clone();
                async move {
                    let futs: Vec<_> = ids
                        .into_iter()
                        .map(|id| {
                            let store = Arc::clone(&store);
                            async move {
                                let (_, raw) = make_raw_events(1);
                                store
                                    .publish(&id, PublishOptions::default(), raw)
                                    .await
                                    .unwrap();
                            }
                        })
                        .collect();
                    join_all(futs).await;
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
        // Pre-provision: all aggregates share the "concentrated" type.
        let ids: Vec<_> = (0..n).map(|_| make_concentrated_id()).collect();
        for id in &ids {
            seed_aggregate(rt, &**store, id, 10);
        }

        group.bench_function(format!("{n}"), |b| {
            b.to_async(rt).iter(|| {
                let store = Arc::clone(store);
                let ids = ids.clone();
                async move {
                    let futs: Vec<_> = ids
                        .into_iter()
                        .map(|id| {
                            let store = Arc::clone(&store);
                            async move {
                                let (_, raw) = make_raw_events(1);
                                store
                                    .publish(&id, PublishOptions::default(), raw)
                                    .await
                                    .unwrap();
                            }
                        })
                        .collect();
                    join_all(futs).await;
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
        let id = make_test_id();
        seed_aggregate(rt, &**store, &id, 1);

        group.bench_function(format!("{n}"), |b| {
            b.to_async(rt).iter(|| {
                let store = Arc::clone(store);
                let id = id.clone();
                async move {
                    let futs: Vec<_> = (0..n)
                        .map(|_| {
                            let store = Arc::clone(&store);
                            let id = id.clone();
                            async move {
                                let (_, raw) = make_raw_events(1);
                                let _ = store.publish(&id, PublishOptions::default(), raw).await;
                            }
                        })
                        .collect();
                    join_all(futs).await;
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
                    let futs: Vec<_> = ids
                        .into_iter()
                        .map(|id| {
                            let store = Arc::clone(&store);
                            async move {
                                store.load(&id).await.unwrap();
                            }
                        })
                        .collect();
                    join_all(futs).await;
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
                    let futs: Vec<_> = ids
                        .into_iter()
                        .map(|id| {
                            let store = Arc::clone(&store);
                            async move {
                                store.load(&id).await.unwrap();
                            }
                        })
                        .collect();
                    join_all(futs).await;
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
        // N readers + N writers = 2N total tasks, interleaved.
        // Even indices read, odd indices write.
        let all_ids: Vec<_> = (0..2 * n).map(make_spread_id).collect();
        for id in &all_ids {
            seed_aggregate(rt, &**store, id, 50);
        }

        group.bench_function(format!("{n}r_{n}w"), |b| {
            b.to_async(rt).iter(|| {
                let store = Arc::clone(store);
                let all_ids = all_ids.clone();
                async move {
                    let futs: Vec<_> = all_ids
                        .into_iter()
                        .enumerate()
                        .map(|(i, id)| {
                            let store = Arc::clone(&store);
                            let is_reader = i % 2 == 0;
                            async move {
                                if is_reader {
                                    store.load(&id).await.unwrap();
                                } else {
                                    let (_, raw) = make_raw_events(1);
                                    store
                                        .publish(&id, PublishOptions::default(), raw)
                                        .await
                                        .unwrap();
                                }
                            }
                        })
                        .collect();
                    join_all(futs).await;
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
            use criterion::{criterion_group, Criterion};
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
