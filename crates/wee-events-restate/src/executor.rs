use std::sync::Arc;

use restate_sdk::prelude::*;

use crate::service::JsonService;
use crate::types::{ExecuteRequest, ExecuteResponse};

pub struct Executor<T> {
    service: Arc<T>,
    service_name: String,
}

impl<T: JsonService> Executor<T> {
    pub fn new(service_name: impl Into<String>, service: Arc<T>) -> Self {
        Self {
            service,
            service_name: service_name.into(),
        }
    }

    pub async fn do_execute(
        &self,
        request: &ExecuteRequest,
    ) -> Result<ExecuteResponse, HandlerError> {
        let result = self
            .service
            .execute(
                &request.command.name,
                &request.command.target,
                request.command.command.clone(),
            )
            .await;

        match result {
            Ok(resp) => Ok(ExecuteResponse {
                aggregate: resp.aggregate_id,
                revision: resp.revision,
                state: resp.state,
            }),
            Err(wee_events::Error::Rejection(r)) => {
                let payload = serde_json::json!({
                    "code": r.code,
                    "message": r.message,
                    "context": r.context,
                });
                Err(TerminalError::new(payload.to_string()).into())
            }
            Err(e) => Err(e.into()),
        }
    }

    pub fn executor_name(&self) -> String {
        format!("{}-side-effect-executor", self.service_name)
    }

    pub fn runner_name(&self) -> String {
        format!("{}-side-effect-runner", self.service_name)
    }
}
