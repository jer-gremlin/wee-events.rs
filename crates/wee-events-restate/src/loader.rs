use std::sync::Arc;

use restate_sdk::prelude::*;

use crate::service::JsonService;
use crate::types::ExecuteResponse;
use wee_events::AggregateId;

pub struct Loader<T> {
    service: Arc<T>,
    service_name: String,
}

impl<T: JsonService> Loader<T> {
    pub fn new(service_name: impl Into<String>, service: Arc<T>) -> Self {
        Self {
            service,
            service_name: service_name.into(),
        }
    }

    pub async fn load(&self, target: AggregateId) -> Result<ExecuteResponse, HandlerError> {
        let resp = self
            .service
            .load(&target)
            .await
            .map_err(|e| TerminalError::new(e.to_string()))?;

        Ok(ExecuteResponse {
            aggregate: resp.aggregate_id,
            revision: resp.revision,
            state: resp.state,
        })
    }

    pub fn loader_name(&self) -> String {
        format!("{}-side-effect-loader", self.service_name)
    }
}
