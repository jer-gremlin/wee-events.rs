use criterion::criterion_main;

wee_events::testing::store_bench_suite!(memory_store, wee_events::memory::MemoryStore::new());

criterion_main!(memory_store::benches);
