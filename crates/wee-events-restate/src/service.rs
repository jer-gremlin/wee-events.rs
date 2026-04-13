use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;

use wee_events::{AggregateId, CommandName, Revision};

#[derive(Debug, Clone)]
pub struct ServiceResponse {
    pub aggregate_id: AggregateId,
    pub revision: Revision,
    pub state: serde_json::Value,
}

type BoxedFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// A type-erased, dyn-compatible service interface.
///
/// Uses explicit `BoxedFuture` returns instead of `async fn` so the trait
/// can be used as `dyn ErasedService` behind an `Arc`.
pub trait ErasedService: Send + Sync {
    fn load<'a>(&'a self, id: &'a AggregateId) -> BoxedFuture<'a, Result<ServiceResponse, wee_events::Error>>;
    fn execute<'a>(
        &'a self,
        name: &'a CommandName,
        target: &'a AggregateId,
        command: serde_json::Value,
    ) -> BoxedFuture<'a, Result<ServiceResponse, wee_events::Error>>;
}

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
    S: Default + serde::Serialize + Send + Sync + 'static,
    T: wee_events::EntityLoader<S> + wee_events::CommandExecutor<S> + Send + Sync + 'static,
{
    fn load<'a>(&'a self, id: &'a AggregateId) -> BoxedFuture<'a, Result<ServiceResponse, wee_events::Error>> {
        Box::pin(async move {
            let entity = self.inner.load(id).await?;
            Ok(ServiceResponse {
                aggregate_id: entity.aggregate_id,
                revision: entity.revision,
                state: serde_json::to_value(&entity.state)?,
            })
        })
    }

    fn execute<'a>(
        &'a self,
        name: &'a CommandName,
        target: &'a AggregateId,
        command: serde_json::Value,
    ) -> BoxedFuture<'a, Result<ServiceResponse, wee_events::Error>> {
        Box::pin(async move {
            let entity = self.inner.execute(name, target, command).await?;
            Ok(ServiceResponse {
                aggregate_id: entity.aggregate_id,
                revision: entity.revision,
                state: serde_json::to_value(&entity.state)?,
            })
        })
    }
}
