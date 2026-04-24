//! Compile-fail witness: a handler whose default wire name is `load`
//! collides with the reserved loader method; the macro must reject this
//! and point at the override syntax.

use wee_events::{AggregateId, Command, Entity, Revision};

#[derive(Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct S;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct Load;
impl Command for Load {
    const NAME: &'static str = "s:load";
}

#[wee_events::loader]
async fn load_state<R: Send + Sync + 'static>(
    _env: &R,
    id: &AggregateId,
) -> wee_events::Result<Entity<S>> {
    Ok(Entity {
        aggregate_id: id.clone(),
        revision: Revision::zero(),
        state: S,
    })
}

#[wee_events::handler(command = Load)]
async fn load<R: Send + Sync + 'static>(
    _env: &R,
    entity: &Entity<S>,
    _cmd: Load,
) -> wee_events::Result<Entity<S>> {
    Ok(entity.clone())
}

wee_events::service! {
    pub Svc("s") for S {
        loader: load_state,
        handlers: [load],
    }
}

fn main() {}
