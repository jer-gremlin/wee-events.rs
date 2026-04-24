//! Proves that `service!` emits a Restate Virtual Object binder whose
//! binding type can be `.serve()`d and `.bind()`ed to a Restate endpoint.
//! We don't hit the wire -- compilation of the builder chain is the
//! assertion.

#![allow(dead_code)]

use wee_events::{AggregateId, Command, Entity, Revision};

#[derive(Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct Counter {
    value: i64,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct Inc;
impl Command for Inc {
    const NAME: &'static str = "counter:increment";
}

#[wee_events::loader]
async fn load<R: Send + Sync + 'static>(
    _env: &R,
    id: &AggregateId,
) -> wee_events::Result<Entity<Counter>> {
    Ok(Entity {
        aggregate_id: id.clone(),
        revision: Revision::zero(),
        state: Counter::default(),
    })
}

#[wee_events::handler(command = Inc)]
async fn inc<R: Send + Sync + 'static>(
    _env: &R,
    e: &Entity<Counter>,
    _cmd: Inc,
) -> wee_events::Result<Entity<Counter>> {
    Ok(e.clone())
}

wee_events::service! {
    pub CounterService("counter") for Counter {
        loader: load,
        handlers: [inc],
    }
}

struct Env;

#[test]
fn binding_can_be_constructed_and_bound() {
    use restate_sdk::prelude::*;
    let binding = CounterService::restate(|| async { Ok(Env) });
    let _endpoint = Endpoint::builder().bind(binding.serve()).build();
}
