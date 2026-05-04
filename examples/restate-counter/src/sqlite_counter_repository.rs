use std::sync::Arc;

use wee_events::{AggregateId, Entity, EventStore, PublishOptions, Revision};
use wee_events_sqlite::SqliteEventStore;

use crate::counter_repository::CounterRepository;
use crate::events::CounterEvent;
use crate::state::{Counter, renderer};

#[derive(Clone)]
pub struct SqliteCounterRepository {
    store: Arc<SqliteEventStore>,
}

impl SqliteCounterRepository {
    pub fn new(store: Arc<SqliteEventStore>) -> Self {
        Self { store }
    }
}

impl CounterRepository for SqliteCounterRepository {
    fn load_counter(
        &self,
        id: &AggregateId,
    ) -> impl std::future::Future<Output = wee_events::Result<Entity<Counter>>> + Send {
        let store = Arc::clone(&self.store);
        let id = id.clone();
        async move {
            let aggregate = store.load(&id).await?;
            renderer().render(&aggregate)
        }
    }

    fn publish_counter_events(
        &self,
        id: AggregateId,
        expected_revision: Revision,
        events: Vec<CounterEvent>,
    ) -> impl std::future::Future<Output = wee_events::Result<Entity<Counter>>> + Send {
        let store = Arc::clone(&self.store);
        async move {
            let raw_events = events
                .iter()
                .map(wee_events::to_raw_event)
                .collect::<wee_events::Result<Vec<_>>>()?;
            store
                .publish(
                    &id,
                    PublishOptions {
                        expected_revision: Some(expected_revision),
                        ..Default::default()
                    },
                    raw_events,
                )
                .await?;
            let aggregate = store.load(&id).await?;
            renderer().render(&aggregate)
        }
    }
}
