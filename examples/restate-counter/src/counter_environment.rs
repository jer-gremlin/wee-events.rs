use wee_events::{AggregateId, Entity, Revision};

use crate::counter_repository::CounterRepository;
use crate::events::CounterEvent;
use crate::randomizer::Randomizer;
use crate::state::Counter;

#[derive(Clone)]
pub struct CounterEnvironment<Repository, Randomness> {
    repository: Repository,
    randomizer: Randomness,
}

impl<Repository, Randomness> CounterEnvironment<Repository, Randomness> {
    pub fn new(repository: Repository, randomizer: Randomness) -> Self {
        Self {
            repository,
            randomizer,
        }
    }
}

impl<Repository, Randomness> CounterRepository for CounterEnvironment<Repository, Randomness>
where
    Repository: CounterRepository,
    Randomness: Send + Sync,
{
    fn load_counter(
        &self,
        id: &AggregateId,
    ) -> impl std::future::Future<Output = wee_events::Result<Entity<Counter>>> + Send {
        self.repository.load_counter(id)
    }

    fn publish_counter_events(
        &self,
        id: AggregateId,
        expected_revision: Revision,
        events: Vec<CounterEvent>,
    ) -> impl std::future::Future<Output = wee_events::Result<Entity<Counter>>> + Send {
        self.repository
            .publish_counter_events(id, expected_revision, events)
    }
}

impl<Repository, Randomness> Randomizer for CounterEnvironment<Repository, Randomness>
where
    Repository: Send + Sync,
    Randomness: Randomizer,
{
    fn random_amount(
        &self,
        min: i64,
        max: i64,
    ) -> impl std::future::Future<Output = wee_events::Result<i64>> + Send {
        self.randomizer.random_amount(min, max)
    }
}
