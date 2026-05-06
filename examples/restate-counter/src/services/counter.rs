use wee_events::{AggregateId, Entity, EventStore, HasPublisher, ServiceError};

use crate::commands::{Increment, Randomise, Reset};
use crate::events::CounterEvent;
use crate::randomiser::Randomiser;
use crate::services::audit_log::AuditLogClient;
use crate::state::{Counter, renderer};

#[wee_events::loader(requires(wee_events::EventStore))]
pub async fn load<R: EventStore>(
    store: &R,
    id: &AggregateId,
) -> Result<Entity<Counter>, ServiceError<R::Error>> {
    let aggregate = store.load(id).await.map_err(ServiceError::Store)?;
    Ok(renderer().render(&aggregate)?)
}

#[wee_events::handler(command = Increment, requires(wee_events::HasPublisher))]
pub async fn increment<R: HasPublisher>(
    env: &R,
    entity: &Entity<Counter>,
    command: Increment,
) -> Result<(), ServiceError<<R::Store as EventStore>::Error>> {
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
pub async fn reset<R: HasPublisher>(
    env: &R,
    entity: &Entity<Counter>,
    _command: Reset,
) -> Result<(), ServiceError<<R::Store as EventStore>::Error>> {
    if entity.state.value == 0 {
        return Ok(());
    }

    env.publisher()
        .publish(entity, vec![CounterEvent::Reset])
        .await?;
    Ok(())
}

#[wee_events::handler(command = Randomise, requires(wee_events::HasPublisher, Randomiser))]
pub async fn randomise<R: HasPublisher + Randomiser>(
    env: &R,
    entity: &Entity<Counter>,
    command: Randomise,
) -> Result<(), ServiceError<<R::Store as EventStore>::Error>> {
    let amount = env.random_amount(command.min, command.max).await;
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
            AuditLog on all,
        ],
    }
}
