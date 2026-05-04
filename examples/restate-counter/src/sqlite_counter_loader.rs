use std::sync::Arc;

use wee_events::{Aggregate, AggregateId, EventStore};
use wee_events_sqlite::SqliteEventStore;

use crate::counter_loader::CounterLoader;

#[derive(Clone)]
pub struct SqliteCounterLoader {
    store: Arc<SqliteEventStore>,
}

impl SqliteCounterLoader {
    pub fn new(store: Arc<SqliteEventStore>) -> Self {
        Self { store }
    }
}

impl CounterLoader for SqliteCounterLoader {
    fn load_counter(
        &self,
        id: &AggregateId,
    ) -> impl std::future::Future<Output = wee_events::Result<Aggregate>> + Send {
        let store = Arc::clone(&self.store);
        let id = id.clone();
        async move { store.load(&id).await }
    }
}
