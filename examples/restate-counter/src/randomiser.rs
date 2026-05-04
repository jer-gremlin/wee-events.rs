#[wee_events::capability]
pub trait Randomiser {
    async fn random_amount(&self, min: i64, max: i64) -> wee_events::Result<i64>;
}

impl<Store, Services> Randomiser for wee_events_restate::HandlerEnv<Store, Services>
where
    Store: Send + Sync,
    Services: Randomiser,
{
    fn random_amount(
        &self,
        min: i64,
        max: i64,
    ) -> impl std::future::Future<Output = wee_events::Result<i64>> + Send {
        self.services().random_amount(min, max)
    }
}
