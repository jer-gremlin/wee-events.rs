use wee_events::{AggregateId, ChangeSet, Revision};

use crate::events::CounterEvent;

pub trait CounterPublisher: Send + Sync {
    fn publish_counter_events(
        &self,
        id: AggregateId,
        expected_revision: Revision,
        events: Vec<CounterEvent>,
    ) -> impl std::future::Future<Output = wee_events::Result<ChangeSet>> + Send;
}
