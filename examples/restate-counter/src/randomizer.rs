#[wee_events::capability]
pub trait Randomizer {
    async fn random_amount(&self, min: i64, max: i64) -> wee_events::Result<i64>;
}
