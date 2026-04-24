//! Proves that `service!` with `effects:` declarations compiles and that
//! the emitted binder type still binds under restate-sdk's endpoint
//! builder. End-to-end wire behaviour is out of scope for a unit test;
//! correctness of the emitted match arms is asserted via compilation.

#![allow(dead_code)]

use wee_events::{AggregateId, Command, Entity, Revision};

#[derive(Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct Counter;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct Inc;
impl Command for Inc {
    const NAME: &'static str = "counter:increment";
}

#[restate_sdk::workflow]
trait Notifier {
    async fn run(
        notification: restate_sdk::serde::Json<wee_events_restate::ExecuteNotification>,
    ) -> Result<(), restate_sdk::errors::HandlerError>;
}

struct NotifierImpl;
impl Notifier for NotifierImpl {
    async fn run(
        &self,
        _ctx: restate_sdk::prelude::WorkflowContext<'_>,
        _n: restate_sdk::serde::Json<wee_events_restate::ExecuteNotification>,
    ) -> Result<(), restate_sdk::errors::HandlerError> {
        Ok(())
    }
}

#[wee_events::loader]
async fn load<R: Send + Sync + 'static>(
    _env: &R,
    id: &AggregateId,
) -> wee_events::Result<Entity<Counter>> {
    Ok(Entity {
        aggregate_id: id.clone(),
        revision: Revision::zero(),
        state: Counter,
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
        effects: [
            Notifier on any,
        ],
    }
}

#[test]
fn binding_compiles_with_effects() {
    use restate_sdk::prelude::*;
    let binding = CounterService::restate(|| async { Ok(()) });
    let _endpoint = Endpoint::builder()
        .bind(binding.serve())
        .bind(NotifierImpl.serve())
        .build();
}
