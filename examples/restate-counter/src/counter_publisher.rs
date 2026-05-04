use wee_events::{AggregateId, ChangeSet, EventStore, PublishOptions, Revision};

use crate::events::CounterEvent;

#[wee_events::capability]
pub trait CounterPublisher: Send + Sync {
    async fn publish_counter_events(
        &self,
        id: AggregateId,
        expected_revision: Revision,
        events: Vec<CounterEvent>,
    ) -> wee_events::Result<ChangeSet>;
}

impl<Store, Services> CounterPublisher for wee_events_restate::HandlerEnv<Store, Services>
where
    Store: EventStore,
    Services: Send + Sync,
{
    async fn publish_counter_events(
        &self,
        id: AggregateId,
        expected_revision: Revision,
        events: Vec<CounterEvent>,
    ) -> wee_events::Result<ChangeSet> {
        let raw_events = events
            .iter()
            .map(wee_events::to_raw_event)
            .collect::<wee_events::Result<Vec<_>>>()?;
        self.store()
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
