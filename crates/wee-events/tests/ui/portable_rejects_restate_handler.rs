//! Compile-fail witness: a service whose handlers declare
//! `requires(HasRestateContext)` cannot be run under the portable
//! interpreter, because the generated service env trait inherits
//! `HasRestateContext` and `()` does not satisfy it.

use wee_events::{AggregateId, Command, Entity, Revision};
use wee_events_restate::HasRestateContext;

#[derive(Default, Clone, serde::Serialize, serde::Deserialize)]
struct Counter;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct Inc;
impl Command for Inc {
    const NAME: &'static str = "c:inc";
}

#[wee_events::loader(requires(HasRestateContext))]
async fn load<R: HasRestateContext>(
    _env: &R,
    id: &AggregateId,
) -> wee_events::Result<Entity<Counter>> {
    Ok(Entity {
        aggregate_id: id.clone(),
        revision: Revision::zero(),
        state: Counter,
    })
}

#[wee_events::handler(command = Inc, requires(HasRestateContext))]
async fn inc<R: HasRestateContext>(
    _env: &R,
    entity: &Entity<Counter>,
    _cmd: Inc,
) -> wee_events::Result<Entity<Counter>> {
    Ok(entity.clone())
}

wee_events::service! {
    pub C("c") for Counter {
        loader: load,
        handlers: [inc],
    }
}

fn main() {
    // Portable factory returns `()`, which cannot satisfy `HasRestateContext`
    // — and therefore cannot satisfy the generated `CEnv` trait.
    let _ = C::portable(|| async { Ok(()) });
}
