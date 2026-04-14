//! Runs the conformance test suite against `MemoryStore`.

wee_events::testing::store_test_suite!(memory_store, wee_events::memory::MemoryStore::new());

wee_events::testing::shared_store_test_suite!(memory_store_shared_backing, {
    let backing = std::sync::Arc::new(wee_events::memory::MemoryStoreBacking::new());
    (
        wee_events::memory::MemoryStore::from_shared(std::sync::Arc::clone(&backing)),
        wee_events::memory::MemoryStore::from_shared(backing),
    )
});
