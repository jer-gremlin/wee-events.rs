use std::sync::Arc;

use restate_sdk::prelude::*;

use crate::names;
use crate::service::JsonService;
use crate::types::EntityResponse;
use wee_events::AggregateId;

pub struct LoadHandler<T> {
    service: Arc<T>,
    service_name: String,
}

impl<T: JsonService> LoadHandler<T> {
    pub fn new(service_name: impl Into<String>, service: Arc<T>) -> Self {
        Self {
            service,
            service_name: service_name.into(),
        }
    }

    pub async fn load(&self, target: AggregateId) -> Result<EntityResponse, HandlerError> {
        let resp = self
            .service
            .load(&target)
            .await
            .map_err(|e| TerminalError::new(e.to_string()))?;

        Ok(EntityResponse {
            aggregate: resp.aggregate,
            revision: resp.revision,
            state: resp.state,
        })
    }

    pub fn loader_name(&self) -> String {
        names::loader_name(&self.service_name)
    }
}
