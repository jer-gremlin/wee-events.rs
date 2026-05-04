use wee_events::{Aggregate, AggregateId, ChangeSet, Revision};

use crate::counter_loader::CounterLoader;
use crate::counter_publisher::CounterPublisher;
use crate::events::CounterEvent;
use crate::randomiser::Randomiser;

#[derive(Clone)]
pub struct CounterEnvironment<Loader, Publisher, Randomness> {
    loader: Loader,
    publisher: Publisher,
    randomiser: Randomness,
}

impl<Loader, Publisher, Randomness> CounterEnvironment<Loader, Publisher, Randomness> {
    pub fn new(loader: Loader, publisher: Publisher, randomiser: Randomness) -> Self {
        Self {
            loader,
            publisher,
            randomiser,
        }
    }
}

impl<Loader, Publisher, Randomness> CounterLoader
    for CounterEnvironment<Loader, Publisher, Randomness>
where
    Loader: CounterLoader,
    Publisher: Send + Sync,
    Randomness: Send + Sync,
{
    fn load_counter(
        &self,
        id: &AggregateId,
    ) -> impl std::future::Future<Output = wee_events::Result<Aggregate>> + Send {
        self.loader.load_counter(id)
    }
}

impl<Loader, Publisher, Randomness> CounterPublisher
    for CounterEnvironment<Loader, Publisher, Randomness>
where
    Loader: Send + Sync,
    Publisher: CounterPublisher,
    Randomness: Send + Sync,
{
    fn publish_counter_events(
        &self,
        id: AggregateId,
        expected_revision: Revision,
        events: Vec<CounterEvent>,
    ) -> impl std::future::Future<Output = wee_events::Result<ChangeSet>> + Send {
        self.publisher
            .publish_counter_events(id, expected_revision, events)
    }
}

impl<Loader, Publisher, Randomness> Randomiser for CounterEnvironment<Loader, Publisher, Randomness>
where
    Loader: Send + Sync,
    Publisher: Send + Sync,
    Randomness: Randomiser,
{
    fn random_amount(
        &self,
        min: i64,
        max: i64,
    ) -> impl std::future::Future<Output = wee_events::Result<i64>> + Send {
        self.randomiser.random_amount(min, max)
    }
}
