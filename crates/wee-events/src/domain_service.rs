use crate::dispatcher::Dispatcher;
use crate::entity::Entity;
use crate::event::DeserializeJsonError;
use crate::id::{AggregateId, CommandName};
use crate::renderer::Renderer;
use crate::service::{CommandExecutor, EntityLoader, ServiceError};
use crate::store::{EventStore, PublishOptions};

/// Composes a `Dispatcher`, `EventStore`, `Renderer`, and context into a
/// full `Service<S>` implementation.
///
/// On execute:
/// 1. Loads the current entity state via the store + renderer
/// 2. Dispatches the command to the matching handler
/// 3. Publishes the resulting events with optimistic concurrency
/// 4. Reloads and returns the updated entity
pub struct DomainService<Ctx, S, Store> {
    ctx: Ctx,
    dispatcher: Dispatcher<Ctx, S>,
    store: Store,
    renderer: Renderer<S>,
}

impl<Ctx, S, Store> DomainService<Ctx, S, Store> {
    pub fn new(
        ctx: Ctx,
        dispatcher: Dispatcher<Ctx, S>,
        store: Store,
        renderer: Renderer<S>,
    ) -> Self {
        Self {
            ctx,
            dispatcher,
            store,
            renderer,
        }
    }
}

impl<Ctx, S, Store> EntityLoader<S> for DomainService<Ctx, S, Store>
where
    Ctx: Send + Sync,
    S: Default + Send + Sync,
    Store: EventStore,
{
    type Error = Store::Error;

    async fn load(&self, id: &AggregateId) -> Result<Entity<S>, Store::Error> {
        let aggregate = self.store.load(id).await?;
        self.renderer
            .render(&aggregate)
            .map_err(DeserializeJsonError::into_store_error)
    }
}

impl<Ctx, S, Store> CommandExecutor<S> for DomainService<Ctx, S, Store>
where
    Ctx: Send + Sync + 'static,
    S: Default + Send + Sync + 'static,
    Store: EventStore,
{
    type Error = ServiceError<Store::Error>;

    async fn execute(
        &self,
        name: &CommandName,
        target: &AggregateId,
        command: serde_json::Value,
    ) -> Result<Entity<S>, ServiceError<Store::Error>> {
        let entity = <Self as EntityLoader<S>>::load(self, target)
            .await
            .map_err(ServiceError::Store)?;

        let events = self
            .dispatcher
            .dispatch(&self.ctx, &entity, name, command)
            .await
            .map_err(ServiceError::Rejection)?;

        if events.is_empty() {
            return Ok(entity);
        }

        let options = PublishOptions {
            expected_revision: Some(entity.revision.clone()),
            ..Default::default()
        };
        self.store
            .publish(target, options, events)
            .await
            .map_err(ServiceError::Store)?;

        <Self as EntityLoader<S>>::load(self, target)
            .await
            .map_err(ServiceError::Store)
    }
}
