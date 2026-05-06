use std::sync::Arc;

use restate_sdk::prelude::*;

use crate::error::Error;
use crate::names;
use crate::service::JsonService;
use crate::types::{EntityResponse, ExecuteRequest};

pub struct CommandHandler<T> {
    service: Arc<T>,
    service_name: String,
}

impl<T: JsonService> CommandHandler<T> {
    pub fn new(service_name: impl Into<String>, service: Arc<T>) -> Self {
        Self {
            service,
            service_name: service_name.into(),
        }
    }

    pub async fn do_execute(
        &self,
        request: &ExecuteRequest,
    ) -> Result<EntityResponse, HandlerError> {
        let result = self
            .service
            .execute(
                &request.command.name,
                &request.command.target,
                request.command.command.clone(),
            )
            .await;

        match result {
            Ok(resp) => Ok(EntityResponse {
                aggregate: resp.aggregate,
                revision: resp.revision,
                state: resp.state,
            }),
            Err(Error::Rejection(r)) => {
                let payload = serde_json::json!({
                    "code": r.code,
                    "message": r.message,
                    "context": r.context,
                });
                Err(TerminalError::new(payload.to_string()).into())
            }
            // Decode and store errors are deterministic — don't retry.
            Err(Error::Decode(e)) => Err(TerminalError::new(e.to_string()).into()),
            Err(Error::Store(e)) => Err(TerminalError::new(e.to_string()).into()),
            // Transport and backend errors are transient — let Restate retry.
            Err(Error::Transport(e)) => Err(HandlerError::from(e)),
            Err(Error::Backend(s)) => Err(HandlerError::from(std::io::Error::other(s))),
        }
    }

    pub fn executor_name(&self) -> String {
        names::executor_name(&self.service_name)
    }

    pub fn runner_name(&self) -> String {
        names::runner_name(&self.service_name)
    }
}
