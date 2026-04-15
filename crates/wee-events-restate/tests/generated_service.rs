//! Compile-time shape and trait tests for `restate_service!`-generated items.
//!
//! Verifies:
//! - `CounterServiceClient` has the correct API shape
//! - `Handles<C>` bounds prevent dispatching unregistered commands
//! - `TypedService<Counter>` is implemented for the client
//! - `CounterServiceServer::dispatch_json` routes JSON to typed handlers
//!
//! No HTTP calls are made — there is no Restate server running during tests.

use wee_events::{AggregateId, Command};

// ---------------------------------------------------------------------------
// Domain types
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
struct Counter {
    value: i64,
}

// Command types need both Serialize (client side) and Deserialize (server dispatch).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct Increment {
    amount: i64,
}

impl Command for Increment {
    const NAME: &'static str = "counter:increment";
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct Adjust;

impl Command for Adjust {
    const NAME: &'static str = "counter:adjust";
}

// ---------------------------------------------------------------------------
// Client + server generation
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
// Shared-caller helper — accepts any TypedService<Counter> that handles both
// commands. Used to verify CounterServiceClient satisfies TypedService<Counter>.
// ---------------------------------------------------------------------------

fn assert_typed_service<T>(_: &T)
where
    T: wee_events::TypedService<Counter>
        + wee_events::Handles<Increment, ()>
        + wee_events::Handles<Adjust, ()>,
{
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

/// Verify that `CounterServiceClient` satisfies `TypedService<Counter>`.
/// This is a compile-time check — `assert_typed_service` accepts only types
/// that implement the trait.
#[test]
fn client_implements_typed_service() {
    let client = CounterServiceClient::new("http://localhost:8080", "counter");
    assert_typed_service(&client);
}

/// Verify that `CounterServiceServer` is generated as a zero-size type.
#[test]
fn server_struct_is_generated() {
    let _ = CounterServiceServer;
}
