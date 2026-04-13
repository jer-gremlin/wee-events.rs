use restate_sdk::prelude::*;

use wee_events_restate::ExecuteNotification;

#[restate_sdk::workflow]
#[name = "logging-effect"]
pub trait LoggingEffect {
    async fn run(notification: Json<ExecuteNotification>) -> Result<(), HandlerError>;
}

pub struct AuditLogger;

impl LoggingEffect for AuditLogger {
    async fn run(
        &self,
        ctx: WorkflowContext<'_>,
        notification: Json<ExecuteNotification>,
    ) -> Result<(), HandlerError> {
        let n = notification.into_inner();
        let command_name = &n.command.name;
        let target = &n.command.target;
        let correlation_id = &n.metadata.correlation_id;

        ctx.run(|| async move {
            println!(
                "[side-effect] command={command_name} target={target} correlation={correlation_id}"
            );
            Ok(())
        })
        .await?;

        Ok(())
    }
}
