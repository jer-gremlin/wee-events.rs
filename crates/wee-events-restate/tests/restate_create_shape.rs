use restate_sdk::prelude::*;
use serde::{Deserialize, Serialize};
use wee_events::{AggregateId, Command, Entity, Revision};

#[wee_events::capability]
pub trait Randomizer {
    async fn amount(&self, min: i64, max: i64) -> wee_events::Result<i64>;
}

#[derive(Clone)]
struct FixedRandomizer;

impl Randomizer for FixedRandomizer {
    async fn amount(&self, _min: i64, max: i64) -> wee_events::Result<i64> {
        Ok(max)
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct Counter {
    value: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Randomise {
    min: i64,
    max: i64,
}

impl Command for Randomise {
    const NAME: &'static str = "counter:randomise";
}

#[wee_events::loader]
async fn load<R: Send + Sync>(_env: &R, id: &AggregateId) -> wee_events::Result<Entity<Counter>> {
    Ok(Entity {
        aggregate_id: id.clone(),
        revision: Revision::zero(),
        state: Counter::default(),
    })
}

#[wee_events::handler(command = Randomise, requires(Randomizer))]
async fn randomise<R: Randomizer>(
    env: &R,
    entity: &Entity<Counter>,
    command: Randomise,
) -> wee_events::Result<Entity<Counter>> {
    let amount = env.amount(command.min, command.max).await?;
    Ok(Entity {
        aggregate_id: entity.aggregate_id.clone(),
        revision: entity.revision.clone(),
        state: Counter {
            value: entity.state.value + amount,
        },
    })
}

wee_events::service! {
    CounterService("counter") for Counter {
        loader: load,
        handlers: [randomise],
    }
}

#[test]
fn create_binds_services_inside_restate_run_boundary() {
    let binding = wee_events_restate::create(CounterService)
        .with_env(FixedRandomizer)
        .serve();
    let _endpoint = Endpoint::builder().bind(binding).build();
}
