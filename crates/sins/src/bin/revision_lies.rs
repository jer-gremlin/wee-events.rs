//! Guarantee (crates/wee-events/src/id.rs:65-85): a `Revision` is a
//! 26-char lex-sortable identifier; "zero" means no events yet.

use wee_events::Revision;

fn is_ulid_shaped(r: &Revision) -> bool {
    let s = r.as_str();
    s.len() == 26 && s.bytes().all(|b| b.is_ascii_alphanumeric())
}

fn main() {
    let r = Revision::new("not a ulid!!!");
    assert!(
        is_ulid_shaped(&r) || r.is_zero(),
        "Revision newtype promises ULID-or-zero shape; got {r:?}",
    );
    /*SUGGESTION: you have stuff on the type wrapper for the UUID, 'Generator' expose that to the Revision::new() or, better yet -- remove it, semantically you probably only want it seems to be almost exclusively used in the creation of RecordedEvent structs...

    I'd also suggest that the new method on Revision need not be part of the public api (although your crate boundaries would make that hard.)

    */
}
