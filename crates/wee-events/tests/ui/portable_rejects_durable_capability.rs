//! Compile-fail witness: when a handler requires a user-defined
//! capability trait that only the Restate-side env satisfies, the
//! portable factory cannot be used -- `()` does not implement the
//! capability, so the generated env trait rejects it.

use wee_events::{AggregateId, Command, Entity, Revision};

trait HasDurableRandom: Send + Sync + 'static {
    fn next_u64(&self) -> u64;
}

#[derive(Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct Counter;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct Inc;
impl Command for Inc {
    const NAME: &'static str = "c:inc";
}

#[wee_events::loader(requires(HasDurableRandom))]
async fn load<R: HasDurableRandom>(
    _env: &R,
    id: &AggregateId,
) -> wee_events::Result<Entity<Counter>> {
    Ok(Entity {
        aggregate_id: id.clone(),
        revision: Revision::zero(),
        state: Counter,
    })
}

#[wee_events::handler(command = Inc, requires(HasDurableRandom))]
async fn inc<R: HasDurableRandom>(
    _env: &R,
    e: &Entity<Counter>,
    _cmd: Inc,
) -> wee_events::Result<Entity<Counter>> {
    Ok(e.clone())
}

wee_events::service! {
    pub C("c") for Counter {
        loader: load,
        handlers: [inc],
    }
}

fn main() {
    // `()` does not implement `HasDurableRandom` -- fails to satisfy `CEnv`.
    let _ = C::portable(|| async { Ok(()) });
}
