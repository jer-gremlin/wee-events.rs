use restate_sdk::{errors::HandlerError, prelude::WorkflowContext};
use serde::{Deserialize, Serialize};
use wee_events::{AggregateId, Command, Entity, Revision};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Counter {
    pub value: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Increment {
    pub amount: i64,
}
impl Command for Increment {
    const NAME: &'static str = "counter:increment";
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reset;
impl Command for Reset {
    const NAME: &'static str = "counter:reset";
}

pub struct Env;

#[wee_events::loader]
pub async fn load<R: Send + Sync + 'static>(
    _env: &R,
    id: &AggregateId,
) -> wee_events::Result<Entity<Counter>> {
    Ok(Entity {
        aggregate_id: id.clone(),
        revision: Revision::zero(),
        state: Counter::default(),
    })
}

#[wee_events::handler(command = Increment)]
pub async fn increment<R: Send + Sync + 'static>(
    _env: &R,
    entity: &Entity<Counter>,
    cmd: Increment,
) -> wee_events::Result<Entity<Counter>> {
    Ok(Entity {
        aggregate_id: entity.aggregate_id.clone(),
        revision: entity.revision.clone(),
        state: Counter {
            value: entity.state.value + cmd.amount,
        },
    })
}

#[wee_events::handler(command = Reset)]
pub async fn reset<R: Send + Sync + 'static>(
    _env: &R,
    entity: &Entity<Counter>,
    _cmd: Reset,
) -> wee_events::Result<Entity<Counter>> {
    Ok(Entity {
        aggregate_id: entity.aggregate_id.clone(),
        revision: entity.revision.clone(),
        state: Counter::default(),
    })
}

#[restate_sdk::workflow]
pub trait AuditLog {
    async fn run(
        notification: restate_sdk::serde::Json<wee_events_restate::ExecuteNotification>,
    ) -> Result<(), HandlerError>;
}

pub struct AuditLogImpl;

impl AuditLog for AuditLogImpl {
    async fn run(
        &self,
        _ctx: WorkflowContext<'_>,
        notification: restate_sdk::serde::Json<wee_events_restate::ExecuteNotification>,
    ) -> Result<(), HandlerError> {
        let notification = notification.into_inner();
        println!(
            "[audit] {} on {} revision {}",
            notification.command.name,
            notification.response.aggregate,
            notification.response.revision
        );
        Ok(())
    }
}

wee_events::service! {
    pub CounterService("counter") for Counter {
        loader: load,
        handlers: [increment, reset],
        effects: [
            AuditLog on any,
        ],
    }
}
