//! I think the ordering can be too easily fucked up on the dev side to create an invalid list of events..
use wee_events::{
    AggregateId, EventData, EventStore, PublishOptions, RawEvent, Revision, memory::MemoryStore,
};

fn raw(t: &str) -> RawEvent {
    RawEvent {
        event_type: t.into(),
        data: EventData::raw("application/json", b"null".to_vec()),
    }
}

fn guarded(r: Revision) -> PublishOptions {
    PublishOptions {
        expected_revision: Some(r),
        ..Default::default()
    }
}

async fn seed_closed(store: &MemoryStore, id: &AggregateId) -> Revision {
    let r1 = store
        .publish(id, PublishOptions::default(), vec![raw("door:opened")])
        .await
        .unwrap()
        .revision;

    store
        .publish(id, guarded(r1), vec![raw("door:closed")])
        .await
        .unwrap()
        .revision
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let store = MemoryStore::new();

    // happypath
    let id = AggregateId::new("door", "happy");
    let r2 = seed_closed(&store, &id).await;
    let r3 = store
        .publish(&id, guarded(r2.clone()), vec![raw("door:opened")])
        .await
        .unwrap()
        .revision;
    let loser = store
        .publish(&id, guarded(r2.clone()), vec![raw("door:locked")])
        .await;
    assert!(r3 > r2, "monotonic revisions");
    assert!(loser.is_err(), "happypath: B's stale write rejected ✓");

    // here-be-dragons
    let id = AggregateId::new("door", "dragons");
    let r2 = seed_closed(&store, &id).await;
    let r3 = store
        .publish(&id, PublishOptions::default(), vec![raw("door:opened")])
        .await
        .unwrap()
        .revision;
    let r4_result = store
        .publish(&id, PublishOptions::default(), vec![raw("door:locked")])
        .await;
    let r4 = r4_result.as_ref().map(|c| c.revision.clone()).ok();

    // Monotonicity holds — the generator guarantees it regardless of guards.
    assert!(r3 > r2, "monotonic revisions");
    if let Some(r4) = &r4 {
        assert!(*r4 > r3, "monotonic revisions");
    }

    assert!(
        r4_result.is_err(),
        "here-be-dragons: B's stale write committed at r4={r4:?}\n  \
         A wrote at r3 based on r2; B wrote at r4 ALSO based on r2.\n  \
         Framework should have flagged the concurrent modification — \
         no guard was passed, so it didn't. Lost update shipped.",
    );
}
