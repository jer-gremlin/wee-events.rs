use restate_sdk::{errors::HandlerError, prelude::WorkflowContext};

#[restate_sdk::workflow]
pub trait AuditLog {
    async fn run(
        notification: restate_sdk::serde::Json<wee_events_restate::ExecuteNotification>,
    ) -> Result<(), HandlerError>;
}

pub struct ConsoleAuditLog;

impl AuditLog for ConsoleAuditLog {
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
