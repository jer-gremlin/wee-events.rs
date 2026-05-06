use restate_sdk::prelude::*;
use serde::{Deserialize, Serialize};
use wee_events::{
    Aggregate, AggregateId, ChangeSet, Command, DomainEvent, Entity, EventStore, PublishOptions,
    RawEvent, Revision,
};

pub trait CounterStore: Send + Sync {
    fn load(
        &self,
        id: &AggregateId,
    ) -> impl std::future::Future<Output = wee_events::Result<Entity<Counter>>> + Send;
}

#[derive(Clone)]
struct FixedStore;

impl CounterStore for FixedStore {
    fn load(
        &self,
        id: &AggregateId,
    ) -> impl std::future::Future<Output = wee_events::Result<Entity<Counter>>> + Send {
        let id = id.clone();
        async move {
            Ok(Entity {
                aggregate_id: id,
                revision: Revision::zero(),
                state: Counter::default(),
            })
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FixedStoreError {
    #[error(transparent)]
    WeeEvents(#[from] wee_events::Error),
    #[error(transparent)]
    Serialization(#[from] serde_json::Error),
}

impl wee_events::EventStoreErrorExt for FixedStoreError {
    fn as_wee_events(&self) -> Option<&wee_events::Error> {
        match self {
            FixedStoreError::WeeEvents(e) => Some(e),
            _ => None,
        }
    }
}

impl EventStore for FixedStore {
    type Error = FixedStoreError;

    async fn load(&self, id: &AggregateId) -> Result<Aggregate, FixedStoreError> {
        Ok(Aggregate::empty(id.clone()))
    }

    async fn publish(
        &self,
        aggregate_id: &AggregateId,
        _options: PublishOptions,
        _events: Vec<RawEvent>,
    ) -> Result<ChangeSet, FixedStoreError> {
        Ok(ChangeSet {
            aggregate_id: aggregate_id.clone(),
            revision: Revision::zero(),
            events: Vec::new(),
        })
    }
}

static JSON_ENCODER: wee_events::JsonEncoder = wee_events::JsonEncoder;

impl wee_events::EncodesEvents for FixedStore {
    type Encoder = wee_events::JsonEncoder;

    fn event_encoder(&self) -> &Self::Encoder {
        &JSON_ENCODER
    }
}

#[wee_events::capability]
pub trait Randomizer {
    async fn amount(&self, min: i64, max: i64) -> wee_events::Result<i64>;
}

#[derive(Clone)]
struct FixedRandomizer;

impl Randomizer for FixedRandomizer {
    async fn amount(&self, _min: i64, max: i64) -> wee_events::Result<i64> {
        Ok(max)
    }
}

impl<Store, Services> Randomizer for wee_events_restate::HandlerEnv<Store, Services>
where
    Store: Send + Sync,
    Services: Randomizer,
{
    fn amount(
        &self,
        min: i64,
        max: i64,
    ) -> impl std::future::Future<Output = wee_events::Result<i64>> + Send {
        self.services().amount(min, max)
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct Counter {
    value: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Randomise {
    min: i64,
    max: i64,
}

impl Command for Randomise {
    const NAME: &'static str = "counter:randomise";
}

#[derive(Debug, Clone, Serialize, Deserialize, DomainEvent)]
#[domain_event(prefix = "counter")]
enum CounterEvent {
    Randomised { amount: i64 },
}

#[wee_events::loader(requires(CounterStore))]
async fn load<R: CounterStore>(store: &R, id: &AggregateId) -> wee_events::Result<Entity<Counter>> {
    store.load(id).await
}

#[wee_events::handler(command = Randomise, requires(wee_events::HasPublisher, Randomizer))]
async fn randomise<R: wee_events::HasPublisher + Randomizer>(
    env: &R,
    entity: &Entity<Counter>,
    command: Randomise,
) -> wee_events::Result<()> {
    let amount = env.amount(command.min, command.max).await?;
    env.publisher()
        .publish(entity, vec![CounterEvent::Randomised { amount }])
        .await
        .expect("publish should succeed");
    Ok(())
}

wee_events::service! {
    CounterService("counter") for Counter {
        loader: load,
        handlers: [randomise],
    }
}

#[test]
fn create_binds_services_inside_restate_run_boundary() {
    let binding = wee_events_restate::create(CounterService)
        .with_store(FixedStore)
        .with_env(FixedRandomizer)
        .serve();
    let _endpoint = Endpoint::builder().bind(binding).build();
}
