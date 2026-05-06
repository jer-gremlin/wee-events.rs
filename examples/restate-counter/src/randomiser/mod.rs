pub mod system;

#[wee_events::capability]
pub trait Randomiser {
    async fn random_amount(&self, min: i64, max: i64) -> i64;
}

impl<Store, Services> Randomiser for wee_events_restate::HandlerEnv<Store, Services>
where
    Store: Send + Sync,
    Services: Randomiser,
{
    fn random_amount(&self, min: i64, max: i64) -> impl std::future::Future<Output = i64> + Send {
        self.services().random_amount(min, max)
    }
}
