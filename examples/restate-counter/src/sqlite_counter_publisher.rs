use std::sync::Arc;

use wee_events::{AggregateId, ChangeSet, EventStore, PublishOptions, Revision};
use wee_events_sqlite::SqliteEventStore;

use crate::counter_publisher::CounterPublisher;
use crate::events::CounterEvent;

#[derive(Clone)]
pub struct SqliteCounterPublisher {
    store: Arc<SqliteEventStore>,
}

impl SqliteCounterPublisher {
    pub fn new(store: Arc<SqliteEventStore>) -> Self {
        Self { store }
    }
}

impl CounterPublisher for SqliteCounterPublisher {
    fn publish_counter_events(
        &self,
        id: AggregateId,
        expected_revision: Revision,
        events: Vec<CounterEvent>,
    ) -> impl std::future::Future<Output = wee_events::Result<ChangeSet>> + Send {
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
                .await
        }
    }
}
