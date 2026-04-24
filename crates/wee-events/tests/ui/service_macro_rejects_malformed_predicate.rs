//! Compile-fail witness: `predicate(...)` must reject extra tokens inside the
//! parenthesized filter instead of silently accepting the first closure.

use wee_events::{AggregateId, Command, Entity, Revision};

#[derive(Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct S;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct Inc;
impl Command for Inc {
    const NAME: &'static str = "s:inc";
}

struct AuditLog;

#[wee_events::loader]
async fn load<R: Send + Sync + 'static>(
    _env: &R,
    id: &AggregateId,
) -> wee_events::Result<Entity<S>> {
    Ok(Entity {
        aggregate_id: id.clone(),
        revision: Revision::zero(),
        state: S,
    })
}

#[wee_events::handler(command = Inc)]
async fn inc<R: Send + Sync + 'static>(
    _env: &R,
    entity: &Entity<S>,
    _cmd: Inc,
) -> wee_events::Result<Entity<S>> {
    Ok(entity.clone())
}

wee_events::service! {
    pub Svc("s") for S {
        loader: load,
        handlers: [inc],
        effects: [
            AuditLog on predicate(|n| true, extra),
        ],
    }
}

fn main() {}
