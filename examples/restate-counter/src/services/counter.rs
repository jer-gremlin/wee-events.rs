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
) -> wee_events::Result<Entity<Counter>>
where
    R::Error: wee_events::EventStoreErrorExt,
{
    let aggregate = store.load(id).await.map_err(flatten_store_error)?;
    renderer().render(&aggregate)
}

/// Flattens a store-specific error into the structural `wee_events::Error`
/// surface expected by the macro-generated loader bridge. Backend variants
/// without a structural origin collapse to `EncodingMismatch` so they remain
/// terminal rather than retried.
fn flatten_store_error<E>(e: E) -> wee_events::Error
where
    E: wee_events::EventStoreErrorExt + std::fmt::Display,
{
    match e.as_wee_events() {
        Some(wee_events::Error::RevisionConflict { expected, actual }) => {
            wee_events::Error::RevisionConflict {
                expected: expected.clone(),
                actual: actual.clone(),
            }
        }
        Some(wee_events::Error::EncodingMismatch { expected, actual }) => {
            wee_events::Error::EncodingMismatch {
                expected: expected.clone(),
                actual: actual.clone(),
            }
        }
        Some(wee_events::Error::RetryExhausted {
            attempts,
            diagnostics,
        }) => wee_events::Error::RetryExhausted {
            attempts: *attempts,
            diagnostics: diagnostics.clone(),
        },
        None => wee_events::Error::EncodingMismatch {
            expected: "successful store call".into(),
            actual: format!("backend error: {e}"),
        },
    }
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
        .await
        .map_err(flatten_publish_error)?;
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
        .await
        .map_err(flatten_publish_error)?;
    Ok(())
}

#[wee_events::handler(command = Randomise, requires(wee_events::HasPublisher, Randomiser))]
pub async fn randomise<R: wee_events::HasPublisher + Randomiser>(
    env: &R,
    entity: &Entity<Counter>,
    command: Randomise,
) -> wee_events::Result<()> {
    let amount = env.random_amount(command.min, command.max).await;
    env.publisher()
        .publish(entity, vec![CounterEvent::Randomised { amount }])
        .await
        .map_err(flatten_publish_error)?;
    Ok(())
}

/// Flattens a publisher `ServiceError` into the structural `wee_events::Error`
/// surface expected by macro-generated dispatch.
///
/// The `service!` macro currently constrains handler return types to
/// `wee_events::Result<()>` and surfaces failures via `to_handler_error`.
/// Store failures are flowed through via the backend's `EventStoreErrorExt`
/// view when the underlying error wraps a structural `wee_events::Error`;
/// other backend variants and codec failures collapse to `EncodingMismatch`
/// with a descriptive message so they remain terminal rather than retried.
fn flatten_publish_error<E>(e: wee_events::ServiceError<E>) -> wee_events::Error
where
    E: wee_events::EventStoreErrorExt + std::error::Error + Send + Sync + 'static,
{
    match e {
        wee_events::ServiceError::Store(inner) => flatten_store_error(inner),
        wee_events::ServiceError::Codec(err) => wee_events::Error::EncodingMismatch {
            expected: "valid event JSON".into(),
            actual: format!("encode failure: {err}"),
        },
        wee_events::ServiceError::Rejection(r) => wee_events::Error::EncodingMismatch {
            expected: "no rejection".into(),
            actual: format!("rejection: {r}"),
        },
    }
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
