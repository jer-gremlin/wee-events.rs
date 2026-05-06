use crate::entity::Entity;
use crate::event::{ChangeSet, DomainEvent};
use crate::service::ServiceError;
use crate::store::{EventStore, PublishOptions, RawEvent};
use crate::EventEncoder;

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
    Store: EventStore + crate::EncodesEvents,
{
    pub async fn publish<S, E>(
        &self,
        entity: &Entity<S>,
        events: Vec<E>,
    ) -> Result<ChangeSet, ServiceError<Store::Error>>
    where
        E: DomainEvent,
    {
        let encoder = self.store.event_encoder();
        let raw_events = events
            .iter()
            .map(|event| {
                Ok(RawEvent {
                    event_type: event.event_type(),
                    data: encoder.serialize(event)?,
                })
            })
            .collect::<Result<Vec<_>, crate::EncodeError>>()
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
    type Store: EventStore + crate::EncodesEvents;

    fn publisher(&self) -> Publisher<'_, Self::Store>;
}
