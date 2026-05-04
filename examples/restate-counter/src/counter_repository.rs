use wee_events::{AggregateId, Entity, Revision};

use crate::events::CounterEvent;
use crate::state::Counter;

pub trait CounterRepository: Send + Sync {
    fn load_counter(
        &self,
        id: &AggregateId,
    ) -> impl std::future::Future<Output = wee_events::Result<Entity<Counter>>> + Send;

    fn publish_counter_events(
        &self,
        id: AggregateId,
        expected_revision: Revision,
        events: Vec<CounterEvent>,
    ) -> impl std::future::Future<Output = wee_events::Result<Entity<Counter>>> + Send;
}
