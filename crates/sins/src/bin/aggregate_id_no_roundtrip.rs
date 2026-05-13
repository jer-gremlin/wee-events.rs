//! Guarantee (crates/wee-events/src/id.rs:135-167): `AggregateId`'s
//! `Display` and `FromStr` round-trip. 

use std::str::FromStr;
use wee_events::AggregateId;

fn main() {
    let id = AggregateId::new("door:has:colons", "front");
    let parsed = AggregateId::from_str(&id.to_string()).unwrap();
    assert!(
        id == parsed,
        "AggregateId Display/FromStr should round-trip\n  expected: {id:?}\n  got     : {parsed:?}\n  (note: Display matches — `{id}` — but the (type, key) split differs)",
    );
    /*SUGGESTIONS:
    - I dunno what the actual behaviour SHOULD be, presumably this however shouldn't be allowed? isn't the point of the agg's ID that it can be reused elsehwere programatically?

    */
}
