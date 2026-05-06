#![allow(unexpected_cfgs)]

use restate_sdk::prelude::*;
use wee_events::{AggregateId, Command, Entity};

trait CounterStore: Send + Sync {
    fn load(
        &self,
        id: &AggregateId,
    ) -> impl std::future::Future<Output = wee_events::Result<Entity<Counter>>> + Send;
}

#[derive(Default, Clone, serde::Serialize, serde::Deserialize)]
struct Counter;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct Inc;
impl Command for Inc {
    const NAME: &'static str = "counter:inc";
}

#[wee_events::loader(requires(CounterStore))]
async fn load<R: CounterStore>(
    store: &R,
    id: &AggregateId,
) -> wee_events::Result<Entity<Counter>> {
    store.load(id).await
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
    }
}

#[derive(Clone)]
struct Env;

fn main() {
    let _endpoint = wee_events_restate::create(CounterService)
        .with_env(Env)
        .attach_to(Endpoint::builder())
        .build();
}
