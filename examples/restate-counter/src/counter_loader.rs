use wee_events::{Aggregate, AggregateId};

pub trait CounterLoader: Send + Sync {
    fn load_counter(
        &self,
        id: &AggregateId,
    ) -> impl std::future::Future<Output = wee_events::Result<Aggregate>> + Send;
}
