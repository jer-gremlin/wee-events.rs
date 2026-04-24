//! Verifies that `service!` accepts an optional `effects:` block with
//! all three filter forms. Emission is Task 8; this is parse-only.

#![allow(dead_code)]

use wee_events::{AggregateId, Command, Entity, Revision};

#[derive(Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct Counter;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct Inc;
impl Command for Inc {
    const NAME: &'static str = "counter:increment";
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct Adj;
impl Command for Adj {
    const NAME: &'static str = "counter:adjust";
}

// User-facing marker types - the macro treats them as workflow idents and
// will emit `<Ident>Client` references in Task 8.
struct SendWelcomeEmail;
struct UpdateAnalytics;
struct AuditLog;

#[wee_events::loader]
async fn load<R: Send + Sync + 'static>(
    _env: &R,
    id: &AggregateId,
) -> wee_events::Result<Entity<Counter>> {
    Ok(Entity {
        aggregate_id: id.clone(),
        revision: Revision::zero(),
        state: Counter,
    })
}

#[wee_events::handler(command = Inc)]
async fn inc<R: Send + Sync + 'static>(
    _env: &R,
    e: &Entity<Counter>,
    _cmd: Inc,
) -> wee_events::Result<Entity<Counter>> {
    Ok(e.clone())
}

#[wee_events::handler(command = Adj)]
async fn adj<R: Send + Sync + 'static>(
    _env: &R,
    e: &Entity<Counter>,
    _cmd: Adj,
) -> wee_events::Result<Entity<Counter>> {
    Ok(e.clone())
}

wee_events::service! {
    pub CounterService("counter") for Counter {
        loader: load,
        handlers: [inc, adj],
        effects: [
            SendWelcomeEmail on [Inc],
            UpdateAnalytics on any,
            AuditLog on predicate(|n| n.command.name.as_str() == "counter:increment"),
        ],
    }
}

#[test]
fn parses_and_still_emits_definition_traits() {
    assert_eq!(
        <CounterService as wee_events::ServiceDefinition>::SERVICE_NAME,
        "counter"
    );
}
