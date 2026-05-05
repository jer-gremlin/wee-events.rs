use wee_events::{AggregateId, Entity};

use crate::commands::{Increment, Randomise, Reset};
use crate::events::CounterEvent;
use crate::randomiser::Randomiser;
use crate::services::audit_log::AuditLogClient;
use crate::state::{Counter, renderer};

#[wee_events::loader(requires(wee_events::EventStore))]
pub async fn load<R: wee_events::EventStore>(
    store: &R,
    id: &AggregateId,
) -> wee_events::Result<Entity<Counter>> {
    let aggregate = store.load(id).await?;
    renderer().render(&aggregate)
}

#[wee_events::handler(command = Increment, requires(wee_events::HasPublisher))]
pub async fn increment<R: wee_events::HasPublisher>(
    env: &R,
    entity: &Entity<Counter>,
    command: Increment,
) -> wee_events::Result<()> {
    env.publisher()
        .publish(
            entity,
            vec![CounterEvent::Incremented {
                amount: command.amount,
            }],
        )
        .await?;
    Ok(())
}

#[wee_events::handler(command = Reset, requires(wee_events::HasPublisher))]
pub async fn reset<R: wee_events::HasPublisher>(
    env: &R,
    entity: &Entity<Counter>,
    _command: Reset,
) -> wee_events::Result<()> {
    if entity.state.value == 0 {
        return Ok(());
    }

    env.publisher()
        .publish(entity, vec![CounterEvent::Reset])
        .await?;
    Ok(())
}

#[wee_events::handler(command = Randomise, requires(wee_events::HasPublisher, Randomiser))]
pub async fn randomise<R: wee_events::HasPublisher + Randomiser>(
    env: &R,
    entity: &Entity<Counter>,
    command: Randomise,
) -> wee_events::Result<()> {
    let amount = env.random_amount(command.min, command.max).await?;
    env.publisher()
        .publish(entity, vec![CounterEvent::Randomised { amount }])
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
