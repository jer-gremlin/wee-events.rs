use rand::Rng;
use restate_sdk::prelude::*;

/// Capability trait — handlers that need randomness constrain their
/// context with `Ctx: HasRandomSource`.
pub trait HasRandomSource {
    fn random_in_range(&self, min: i64, max: i64) -> i64;
}

/// Context carrying a pre-resolved random seed. The seed is fetched
/// durably via Restate (either `ctx.run()` or a service call) before
/// dispatch, so the handler logic is deterministic given the seed.
pub struct SeededRandom {
    seed: f64,
}

impl SeededRandom {
    pub fn new(seed: f64) -> Self {
        Self { seed }
    }
}

impl HasRandomSource for SeededRandom {
    fn random_in_range(&self, min: i64, max: i64) -> i64 {
        if min >= max {
            return min;
        }
        min + (((max - min) as f64) * self.seed) as i64
    }
}

// ---------------------------------------------------------------------------
// Restate service: Random number generator
//
// A standalone Restate service that generates random numbers. Calling it
// through ctx.service_client() makes the result durable — on replay,
// Restate returns the journaled value instead of generating a new one.
// ---------------------------------------------------------------------------

#[restate_sdk::service]
#[name = "random"]
pub trait RandomService {
    async fn seed() -> Result<Json<f64>, HandlerError>;
}

pub struct RandomGenerator;

impl RandomService for RandomGenerator {
    async fn seed(
        &self,
        _ctx: Context<'_>,
    ) -> Result<Json<f64>, HandlerError> {
        let value: f64 = rand::rng().random();
        Ok(Json(value))
    }
}
