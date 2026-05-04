use wee_events::{AggregateId, Entity};

use crate::commands::{Increment, Randomise, Reset};
use crate::environment::CounterRepository;
use crate::events::CounterEvent;
use crate::randomizer::Randomizer;
use crate::services::audit_log::AuditLogClient;
use crate::state::Counter;

#[wee_events::loader(requires(CounterRepository))]
pub async fn load<R: CounterRepository>(
    env: &R,
    id: &AggregateId,
) -> wee_events::Result<Entity<Counter>> {
    env.load_counter(id).await
}

#[wee_events::handler(command = Increment, requires(CounterRepository))]
pub async fn increment<R: CounterRepository>(
    env: &R,
    entity: &Entity<Counter>,
    command: Increment,
) -> wee_events::Result<Entity<Counter>> {
    env.publish_counter_events(
        entity.aggregate_id.clone(),
        entity.revision.clone(),
        vec![CounterEvent::Incremented {
            amount: command.amount,
        }],
    )
    .await
}

#[wee_events::handler(command = Reset, requires(CounterRepository))]
pub async fn reset<R: CounterRepository>(
    env: &R,
    entity: &Entity<Counter>,
    _command: Reset,
) -> wee_events::Result<Entity<Counter>> {
    if entity.state.value == 0 {
        return Ok(entity.clone());
    }

    env.publish_counter_events(
        entity.aggregate_id.clone(),
        entity.revision.clone(),
        vec![CounterEvent::Reset],
    )
    .await
}

#[wee_events::handler(command = Randomise, requires(CounterRepository, Randomizer))]
pub async fn randomise<R: CounterRepository + Randomizer>(
    env: &R,
    entity: &Entity<Counter>,
    command: Randomise,
) -> wee_events::Result<Entity<Counter>> {
    let amount = env.random_amount(command.min, command.max).await?;
    env.publish_counter_events(
        entity.aggregate_id.clone(),
        entity.revision.clone(),
        vec![CounterEvent::Randomised { amount }],
    )
    .await
}

wee_events::service! {
    pub CounterService("counter") for Counter {
        loader: load,
        handlers: [increment, reset, randomise],
        effects: [
            AuditLog on any,
        ],
    }
}
