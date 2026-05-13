//! Guarantee (implicit): a publish that returns `Err` writes nothing
//! observable. Reality (crates/wee-events/src/memory_store.rs:154-170):
//! `streams.entry(id).or_default()` runs BEFORE the revision check,
//! leaking a into `enumerate_aggregates()`.

use wee_events::{
    AggregateId, EventData, EventStore, PublishOptions, RawEvent, Revision, memory::MemoryStore,
};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let store = MemoryStore::new();
    let phantom = AggregateId::new("ghost", "never-written");

    let before = store.enumerate_aggregates();

    _ = store
        // Something that should fail..
        .publish(
            &phantom,
            PublishOptions {
                expected_revision: Some(Revision::new("nope")),
                ..Default::default()
            },
            vec![RawEvent {
                event_type: "ghost:appears".into(),
                data: EventData::raw("application/json", b"null".to_vec()),
            }],
        )
        .await
        .expect_err("setup: this publish is supposed to fail");

    let after = store.enumerate_aggregates();
    assert_eq!(
        before, after,
        "failed publish must not change enumerate_aggregates(); ??? feels like it shouldn't"
    );
    /*SUGGESTION:
    -NFI without some kinda state management... which maybe defeats the purpose of this?
    - I'm also not sure in practice, how achieveable this is... if someone's calling `.publish()` in a loop... probably a huge issue -- if the publication of Events is rare... or the calls to `.enumerate_aggregates()` rare, or in practice if someone only ever calles `.enumerate_aggregatess()` after re-reading stuff from some inda long lived datastore...
    */
}
