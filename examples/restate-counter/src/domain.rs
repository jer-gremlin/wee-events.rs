use serde::{Deserialize, Serialize};
use serde_json::json;

use wee_events::{CommandName, Dispatcher, Entity, EventData, EventType, RawEvent, Rejection, Renderer};

use crate::random::HasRandomSource;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct Counter {
    pub value: i64,
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Serialize)]
pub struct Increment {
    pub amount: i64,
}

impl wee_events::Command for Increment {
    fn command_name(&self) -> CommandName {
        CommandName::from("increment")
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Decrement {
    pub amount: i64,
}

impl wee_events::Command for Decrement {
    fn command_name(&self) -> CommandName {
        CommandName::from("decrement")
    }
}

/// No payload — the adjustment is derived from the random source and balance.
#[derive(Debug, Deserialize, Serialize)]
pub struct Adjust;

impl wee_events::Command for Adjust {
    fn command_name(&self) -> CommandName {
        CommandName::from("adjust")
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn order_of_magnitude(value: i64) -> i64 {
    match value.unsigned_abs() {
        0 => 1,
        n => 10_i64.pow((n as f64).log10().ceil() as u32),
    }
}

// ---------------------------------------------------------------------------
// Handlers
//
// Each handler constrains its context with capability traits. The concrete
// context is constructed per-request by the Restate handler, with values
// fetched durably through the Restate context.
// ---------------------------------------------------------------------------

/// Increment with a random bonus scaled to the current balance.
pub async fn increment<C: HasRandomSource>(
    ctx: &C,
    entity: &Entity<Counter>,
    cmd: Increment,
) -> Result<Vec<RawEvent>, Rejection> {
    let bonus = if entity.state.value == 0 {
        0
    } else {
        let mag = order_of_magnitude(entity.state.value);
        ctx.random_in_range(0, mag)
    };
    let total = cmd.amount + bonus;
    Ok(vec![RawEvent {
        event_type: EventType::from("counter:incremented"),
        data: EventData::json(&json!({ "amount": total, "requested": cmd.amount, "bonus": bonus }))
            .unwrap(),
    }])
}

/// Decrement with a random forgiveness scaled to the current balance.
pub async fn decrement<C: HasRandomSource>(
    ctx: &C,
    entity: &Entity<Counter>,
    cmd: Decrement,
) -> Result<Vec<RawEvent>, Rejection> {
    if entity.state.value == 0 {
        return Err(Rejection::new("INSUFFICIENT_VALUE", "balance is zero"));
    }
    let forgiven = {
        let mag = order_of_magnitude(entity.state.value);
        ctx.random_in_range(0, mag)
    };
    let effective = (cmd.amount - forgiven).max(1).min(entity.state.value);
    Ok(vec![RawEvent {
        event_type: EventType::from("counter:decremented"),
        data: EventData::json(
            &json!({ "amount": effective, "requested": cmd.amount, "forgiven": forgiven }),
        )
        .unwrap(),
    }])
}

/// Fully random adjustment based on balance magnitude.
pub async fn adjust<C: HasRandomSource>(
    ctx: &C,
    entity: &Entity<Counter>,
    _cmd: Adjust,
) -> Result<Vec<RawEvent>, Rejection> {
    let magnitude = order_of_magnitude(entity.state.value);
    let draw = ctx.random_in_range(-magnitude, magnitude);

    if draw > 0 {
        Ok(vec![RawEvent {
            event_type: EventType::from("counter:incremented"),
            data: EventData::json(&json!({ "amount": draw })).unwrap(),
        }])
    } else if draw < 0 {
        let amount = draw.unsigned_abs() as i64;
        Ok(vec![RawEvent {
            event_type: EventType::from("counter:decremented"),
            data: EventData::json(&json!({ "amount": amount })).unwrap(),
        }])
    } else {
        Ok(vec![])
    }
}

// ---------------------------------------------------------------------------
// Reducers
// ---------------------------------------------------------------------------

fn reduce_incremented(
    state: &mut Counter,
    _event_type: &EventType,
    data: &EventData,
) -> Result<(), wee_events::Error> {
    let v: serde_json::Value = data.deserialize_json()?;
    state.value += v["amount"].as_i64().unwrap_or(0);
    Ok(())
}

fn reduce_decremented(
    state: &mut Counter,
    _event_type: &EventType,
    data: &EventData,
) -> Result<(), wee_events::Error> {
    let v: serde_json::Value = data.deserialize_json()?;
    state.value -= v["amount"].as_i64().unwrap_or(0);
    Ok(())
}

// ---------------------------------------------------------------------------
// Component builders — assembled by the Restate handler, not by DomainService
// ---------------------------------------------------------------------------

pub type CounterDispatcher = Dispatcher<crate::random::SeededRandom, Counter>;

pub fn build_dispatcher() -> CounterDispatcher {
    Dispatcher::new()
        .handler("increment", increment)
        .handler("decrement", decrement)
        .handler("adjust", adjust)
}

pub fn build_renderer() -> Renderer<Counter> {
    Renderer::new()
        .with("counter:incremented", reduce_incremented)
        .with("counter:decremented", reduce_decremented)
}

// ---------------------------------------------------------------------------
// Typed service — generated client and server dispatch helper
//
// `CounterServiceClient`  — typed Restate ingress client.
//   Use `CounterServiceClient::new(ingress_url, service_name)` to create an
//   instance. Call `.execute(&id, Increment { amount: 5 })` to dispatch a
//   command; the compile-time `Handles<C>` bound rejects unregistered types.
//
// `CounterServiceServer` — zero-size server dispatch helper.
//   Use `CounterServiceServer::dispatch_json(service, name, target, payload)`
//   to route an incoming JSON-encoded command to the appropriate typed handler.
// ---------------------------------------------------------------------------

wee_events_restate::restate_service! {
    pub CounterService for Counter {
        handlers: [
            Increment => increment,
            Decrement => decrement,
            Adjust => adjust,
        ],
    }
}
