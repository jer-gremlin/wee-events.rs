use super::Randomiser;

#[derive(Clone)]
pub struct SystemRandomiser;

impl Randomiser for SystemRandomiser {
    async fn random_amount(&self, min: i64, max: i64) -> i64 {
        let (low, high) = if min <= max { (min, max) } else { (max, min) };
        let span = (high - low + 1) as u128;
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock set before UNIX_EPOCH")
            .as_nanos();
        low + (nanos % span) as i64
    }
}
