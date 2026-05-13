//! Guarantee (crates/wee-events/src/id.rs:224-238): `EventType` is
//! "kebab-case by convention, prefix:variant format."

use wee_events::EventType;

fn looks_kebab(s: &str) -> bool {
    !s.is_empty()
        && s.contains(':')
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b':')
}

fn main() {
    let t = EventType::new(r"Not Kebab Case AT ALL!! ¯\_( ͡❛ ͜ʖ ͡❛)_/¯ ");
    assert!(
        looks_kebab(t.as_str()),
        "EventType promised kebab-case prefix:variant; got {t:?}",
    );
    /* SUGGESTION:
        - parse don't validate, or.. at least validate?

    */
}
