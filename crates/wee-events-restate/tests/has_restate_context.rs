//! Compile-only witness that `HasRestateContext` is a nameable capability trait.
//!
//! No impls are provided here — the point is only to prove the trait exists
//! and can be used as a bound. Actual impls live in downstream services that
//! plug into the Restate runtime.

use wee_events_restate::HasRestateContext;

fn _require<T: HasRestateContext>() {}

#[test]
fn has_restate_context_is_a_nameable_bound() {
    // If this compiles, the trait is reachable from the public API.
    let _: fn() = _require::<DummyForBoundNaming>;
}

struct DummyForBoundNaming;

impl HasRestateContext for DummyForBoundNaming {
    fn restate_context(&self) -> &restate_sdk::context::Context<'_> {
        unreachable!("compile-only witness — never invoked")
    }
}
