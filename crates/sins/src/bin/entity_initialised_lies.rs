//! Guarantee (crates/wee-events/src/entity.rs:13-16): `initialized()`
//! returns true iff the entity "has at least one event."

use wee_events::{AggregateId, Entity, Revision};

fn main() {
    let e = Entity::<()> {
        aggregate_id: AggregateId::new("door", "front"),
        revision: Revision::new(""), // empty != zero, but also no events
        state: (),
    };
    assert!(
        !e.initialized(),
        "Entity with no events but `initialized()` returned true (revision={:?})",
        e.revision,
    );
    /*
    SUGGESTIONS:
    - I recall you saying that an empty list of events is valid.. is this why?
    Is an agg created over the 'nothingness' of an empty list of events valid?


    */
}
