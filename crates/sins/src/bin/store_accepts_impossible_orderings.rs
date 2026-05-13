//! Where does the framework's fortification end and dev responsibility begin?
//!
//! Concurrency:    fortified — `expected_revision` will reject stale writes. (it's just a sloppy uuid increasing with a lock but... whatever)
//! I suspect tho that, no API ties a publish to a renderer.
//!
//! Below: every publish uses a correct revision guard, so concurrency is
//! acceptable. Hoever, the store still commits an event that produces an un-foldable
//! aggregate (Open → Locked, illegal per the door's reducer). So I guess..
//! and I don't have a good answer to this yet -- but you need a way of ensuring that reducers<T> maybe implementes
//! something that guarantees it cannot mess with the lifecycle independant of
//! whatever events have been comitted?
//!
//! Am I understanding the boundary here correcty?
//!
use wee_events::{
    AggregateId,
    EventData,
    EventStore,
    EventType,
    PublishOptions,
    RawEvent,
    Renderer, // Shouldn't the Renderer be a Fold? it is a fold, or a reduce in the comp-sci sense... cmon
    Revision,
    memory::MemoryStore,
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

#[derive(Debug, Default)]
struct DoorState {
    s: &'static str,
}

fn on_opened(s: &mut DoorState, _: &EventType, _: &EventData) -> Result<(), String> {
    match s.s {
        "" | "Closed" => {
            s.s = "Open";
            Ok(())
        }
        other => Err(format!("cannot Open from {other}")),
    }
}
//NOTE EventType just being a String makes me wanna puke a little..
fn on_locked(s: &mut DoorState, _: &EventType, _: &EventData) -> Result<(), String> {
    match s.s {
        "Closed" => {
            s.s = "Locked";
            Ok(())
        }
        other => Err(format!("cannot Lock from {other}")),
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let store = MemoryStore::new();
    let id = AggregateId::new("door", "fortify");

    // Concurrency-correct publish chain. Each call passes the prior revision.
    let r1 = store
        .publish(&id, PublishOptions::default(), vec![raw("door:opened")])
        .await
        .unwrap()
        .revision;
    // Door is Open. A Lock from Open is domain-illegal — but the store
    // has no idea. Publish committed cleanly.
    let _r2 = store
        .publish(&id, guarded(r1), vec![raw("door:locked")])
        .await
        .expect("store committed Lock-from-Open without consulting any lifecycle");

    // After-the-fact: try to render via the lifecycle reducer.
    let renderer = Renderer::<DoorState, String>::new()
        .with(EventType::new("door:opened"), on_opened)
        .with(EventType::new("door:locked"), on_locked);
    let agg = store.load(&id).await.unwrap();
    let result = renderer.render(&agg);

    /*
    SUGGESTIION:
    - I think as a pragmatic fix to this I'd probs enforce that any state(y) stuff be enforced with a Typestate or exhaustive enum approach..
    - maybe a PublishStrict ??/ I think in my case looking at the doors, it may not seem clear but a door state above is not entirely different to an account being in credit/debit/uninitialised/frozen etc.. in that only certain behaviour should be allowed at the most recent state detected.

    */
    assert!(
        result.is_ok(),
        "publish succeeded despite producing an un-renderable aggregate.\n  \
         Concurrency was perfect (guard passed). The framework's fortification\n  \
         stops there: there is no publish path that consults a renderer/lifecycle.\n  \
         Dev is on the hook to wire load → fold → check → publish — every time.\n  \
         render error: {:?}",
        result.err()
    );
}
