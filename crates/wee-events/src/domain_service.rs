use crate::dispatcher::Dispatcher;
use crate::entity::Entity;
use crate::id::{AggregateId, CommandName};
use crate::renderer::Renderer;
use crate::service::{CommandExecutor, EntityLoader};
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
    async fn load(&self, id: &AggregateId) -> crate::Result<Entity<S>> {
        let aggregate = self.store.load(id).await?;
        self.renderer.render(&aggregate)
    }
}

impl<Ctx, S, Store> CommandExecutor<S> for DomainService<Ctx, S, Store>
where
    Ctx: Send + Sync + 'static,
    S: Default + Send + Sync + 'static,
    Store: EventStore,
{
    async fn execute(
        &self,
        name: &CommandName,
        target: &AggregateId,
        command: serde_json::Value,
    ) -> crate::Result<Entity<S>> {
        let entity = self.load(target).await?;

        let events = self
            .dispatcher
            .dispatch(&self.ctx, &entity, name, command)
            .await
            .map_err(crate::Error::Rejection)?;

        if events.is_empty() {
            return Ok(entity);
        }

        let options = PublishOptions {
            expected_revision: Some(entity.revision.clone()),
            ..Default::default()
        };
        self.store.publish(target, options, events).await?;

        self.load(target).await
    }
}
