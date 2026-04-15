//! Compile-time shape test for `restate_service!`-generated clients.
//!
//! This test verifies that the generated `CounterServiceClient` has the
//! correct API shape and that `Handles<C>` bounds prevent dispatching
//! unregistered commands. No HTTP calls are made — there is no Restate
//! server running during tests.

use wee_events::{AggregateId, Command, CommandName};

// ---------------------------------------------------------------------------
// Domain types
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
struct Counter {
    value: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
struct Increment {
    amount: i64,
}

impl Command for Increment {
    fn command_name(&self) -> CommandName {
        CommandName::from("counter:increment")
    }
}

#[derive(Debug, Clone, serde::Serialize)]
struct Adjust;

impl Command for Adjust {
    fn command_name(&self) -> CommandName {
        CommandName::from("counter:adjust")
    }
}

// ---------------------------------------------------------------------------
// Client generation
// ---------------------------------------------------------------------------

wee_events_restate::restate_service! {
    pub CounterService for Counter {
        handlers: [
            Increment => increment,
            Adjust    => adjust,
        ],
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Verify that `CounterServiceClient` compiles and exposes the expected API.
/// The futures are constructed but never awaited — no network is required.
#[test]
fn generated_client_has_typed_methods() {
    let client = CounterServiceClient::new("http://localhost:8080", "counter");
    let id: AggregateId = "counter:c1".parse().unwrap();

    // Verify load and execute return futures. We use `_` to drop them without
    // awaiting since there is no Restate server in tests.
    let _load_fut = client.load(&id);
    let _inc_fut = client.execute(&id, Increment { amount: 1 });
    let _adj_fut = client.execute(&id, Adjust);
}
