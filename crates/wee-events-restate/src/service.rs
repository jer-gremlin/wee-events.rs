use std::marker::PhantomData;

use wee_events::{AggregateId, CommandName, Revision};

use crate::error::Error;

#[derive(Debug, Clone)]
pub struct ServiceResponse {
    pub aggregate: AggregateId,
    pub revision: Revision,
    pub state: serde_json::Value,
}

/// Service interface that operates on JSON values.
///
/// Serializes the state type `S` to JSON at the boundary so Restate
/// components don't need to be generic over the domain state type.
#[allow(async_fn_in_trait)]
pub trait JsonService: Send + Sync {
    async fn load(&self, id: &AggregateId) -> Result<ServiceResponse, Error>;
    async fn execute(
        &self,
        name: &CommandName,
        target: &AggregateId,
        command: serde_json::Value,
    ) -> Result<ServiceResponse, Error>;
}

/// Adapts a concrete `EntityLoader<S> + CommandExecutor<S>` into `JsonService`
/// by serializing state to JSON.
pub struct ServiceAdapter<S, T> {
    inner: T,
    _phantom: PhantomData<S>,
}

impl<S, T> ServiceAdapter<S, T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            _phantom: PhantomData,
        }
    }
}

impl<S, T> JsonService for ServiceAdapter<S, T>
where
    S: Default + serde::Serialize + Send + Sync,
    T: wee_events::EntityLoader<S> + wee_events::CommandExecutor<S>,
    <T as wee_events::EntityLoader<S>>::Error: Into<Error>,
    <T as wee_events::CommandExecutor<S>>::Error: Into<Error>,
{
    async fn load(&self, id: &AggregateId) -> Result<ServiceResponse, Error> {
        let entity = self.inner.load(id).await.map_err(Into::into)?;
        Ok(ServiceResponse {
            aggregate: entity.aggregate_id,
            revision: entity.revision,
            state: serde_json::to_value(&entity.state)?,
        })
    }

    async fn execute(
        &self,
        name: &CommandName,
        target: &AggregateId,
        command: serde_json::Value,
    ) -> Result<ServiceResponse, Error> {
        let entity = self
            .inner
            .execute(name, target, command)
            .await
            .map_err(Into::into)?;
        Ok(ServiceResponse {
            aggregate: entity.aggregate_id,
            revision: entity.revision,
            state: serde_json::to_value(&entity.state)?,
        })
    }
}
