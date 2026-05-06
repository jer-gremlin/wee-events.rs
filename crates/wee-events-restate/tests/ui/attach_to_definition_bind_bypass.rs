#![allow(unexpected_cfgs)]

use restate_sdk::prelude::*;
use wee_events::{AggregateId, Command, Entity, Revision};

#[derive(Default, Clone, serde::Serialize, serde::Deserialize)]
struct Counter;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct Inc;
impl Command for Inc {
    const NAME: &'static str = "counter:inc";
}

#[restate_sdk::workflow]
trait AuditLog {
    async fn run(
        notification: restate_sdk::serde::Json<wee_events_restate::ExecuteNotification>,
    ) -> Result<(), restate_sdk::errors::HandlerError>;
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
    entity: &Entity<Counter>,
    _cmd: Inc,
) -> wee_events::Result<Entity<Counter>> {
    Ok(entity.clone())
}

wee_events::service! {
    CounterService("counter") for Counter {
        loader: load,
        handlers: [inc],
        effects: [
            AuditLog on all,
        ],
    }
}

#[derive(Clone)]
struct Env;

fn main() {
    let binding = <CounterService as wee_events_restate::RestateServiceDefinition>::bind(Env, Env);
    let _endpoint = Endpoint::builder().bind(binding.serve()).build();
}
