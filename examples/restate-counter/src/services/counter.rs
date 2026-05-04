use wee_events::{AggregateId, Entity};

use crate::commands::{Increment, Randomise, Reset};
use crate::counter_loader::CounterLoader;
use crate::counter_publisher::CounterPublisher;
use crate::events::CounterEvent;
use crate::randomiser::Randomiser;
use crate::services::audit_log::AuditLogClient;
use crate::state::{Counter, renderer};

#[wee_events::loader(requires(CounterLoader))]
pub async fn load<R: CounterLoader>(
    env: &R,
    id: &AggregateId,
) -> wee_events::Result<Entity<Counter>> {
    let aggregate = env.load_counter(id).await?;
    renderer().render(&aggregate)
}

#[wee_events::handler(command = Increment, requires(CounterPublisher))]
pub async fn increment<R: CounterPublisher>(
    env: &R,
    entity: &Entity<Counter>,
    command: Increment,
) -> wee_events::Result<()> {
    env.publish_counter_events(
        entity.aggregate_id.clone(),
        entity.revision.clone(),
        vec![CounterEvent::Incremented {
            amount: command.amount,
        }],
    )
    .await?;
    Ok(())
}

#[wee_events::handler(command = Reset, requires(CounterPublisher))]
pub async fn reset<R: CounterPublisher>(
    env: &R,
    entity: &Entity<Counter>,
    _command: Reset,
) -> wee_events::Result<()> {
    if entity.state.value == 0 {
        return Ok(());
    }

    env.publish_counter_events(
        entity.aggregate_id.clone(),
        entity.revision.clone(),
        vec![CounterEvent::Reset],
    )
    .await?;
    Ok(())
}

#[wee_events::handler(command = Randomise, requires(CounterPublisher, Randomiser))]
pub async fn randomise<R: CounterPublisher + Randomiser>(
    env: &R,
    entity: &Entity<Counter>,
    command: Randomise,
) -> wee_events::Result<()> {
    let amount = env.random_amount(command.min, command.max).await?;
    env.publish_counter_events(
        entity.aggregate_id.clone(),
        entity.revision.clone(),
        vec![CounterEvent::Randomised { amount }],
    )
    .await?;
    Ok(())
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
