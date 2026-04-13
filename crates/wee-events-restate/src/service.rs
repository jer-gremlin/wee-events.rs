use std::marker::PhantomData;

use wee_events::{AggregateId, CommandName, Revision};

#[derive(Debug, Clone)]
pub struct ServiceResponse {
    pub aggregate_id: AggregateId,
    pub revision: Revision,
    pub state: serde_json::Value,
}

/// Type-erased service interface that operates on JSON values.
///
/// Erases the state type `S` by serializing to JSON at the boundary.
/// Used with static dispatch — no `dyn` needed.
#[allow(async_fn_in_trait)]
pub trait ErasedService: Send + Sync {
    async fn load(&self, id: &AggregateId) -> Result<ServiceResponse, wee_events::Error>;
    async fn execute(
        &self,
        name: &CommandName,
        target: &AggregateId,
        command: serde_json::Value,
    ) -> Result<ServiceResponse, wee_events::Error>;
}

/// Adapts a concrete `EntityLoader<S> + CommandExecutor<S>` into `ErasedService`
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

impl<S, T> ErasedService for ServiceAdapter<S, T>
where
    S: Default + serde::Serialize + Send + Sync,
    T: wee_events::EntityLoader<S> + wee_events::CommandExecutor<S>,
{
    async fn load(&self, id: &AggregateId) -> Result<ServiceResponse, wee_events::Error> {
        let entity = self.inner.load(id).await?;
        Ok(ServiceResponse {
            aggregate_id: entity.aggregate_id,
            revision: entity.revision,
            state: serde_json::to_value(&entity.state)?,
        })
    }

    async fn execute(
        &self,
        name: &CommandName,
        target: &AggregateId,
        command: serde_json::Value,
    ) -> Result<ServiceResponse, wee_events::Error> {
        let entity = self.inner.execute(name, target, command).await?;
        Ok(ServiceResponse {
            aggregate_id: entity.aggregate_id,
            revision: entity.revision,
            state: serde_json::to_value(&entity.state)?,
        })
    }
}
