use crate::randomiser::Randomiser;

#[derive(Clone)]
pub struct SystemRandomiser;

impl Randomiser for SystemRandomiser {
    async fn random_amount(&self, min: i64, max: i64) -> wee_events::Result<i64> {
        let (low, high) = if min <= max { (min, max) } else { (max, min) };
        let span = (high - low + 1) as u128;
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| wee_events::Error::Store(Box::new(e)))?
            .as_nanos();
        Ok(low + (nanos % span) as i64)
    }
}
