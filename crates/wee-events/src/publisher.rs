use crate::entity::Entity;
use crate::event::{ChangeSet, DomainEvent};
use crate::service::ServiceError;
use crate::store::{EventStore, PublishOptions};

/// Publishes typed domain events for an aggregate entity.
pub struct Publisher<'a, Store> {
    store: &'a Store,
}

impl<'a, Store> Publisher<'a, Store> {
    pub fn new(store: &'a Store) -> Self {
        Self { store }
    }
}

impl<Store> Publisher<'_, Store>
where
    Store: EventStore,
{
    pub async fn publish<S, E>(
        &self,
        entity: &Entity<S>,
        events: Vec<E>,
    ) -> Result<ChangeSet, ServiceError<Store::Error>>
    where
        E: DomainEvent,
    {
        let raw_events = events
            .iter()
            .map(crate::to_raw_event)
            .collect::<Result<Vec<_>, _>>()
            .map_err(ServiceError::Codec)?;

        self.store
            .publish(
                &entity.aggregate_id,
                PublishOptions {
                    expected_revision: Some(entity.revision.clone()),
                    ..Default::default()
                },
                raw_events,
            )
            .await
            .map_err(ServiceError::Store)
    }
}

pub trait HasPublisher: Send + Sync {
    type Store: EventStore;

    fn publisher(&self) -> Publisher<'_, Self::Store>;
}
