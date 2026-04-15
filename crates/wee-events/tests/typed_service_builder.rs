//! Integration tests for the typed `ServiceBuilder` runtime.
//!
//! Verifies that:
//! - The factory is called for both `load` and `execute`.
//! - `execute` dispatches to the correct handler.
//! - Multiple handlers coexist on the same service.
//! - `Handles<C>` bounds are satisfied at compile time.

use std::sync::{Arc, Mutex};

use wee_events::{AggregateId, Entity, Rejection, Revision, ServiceBuilder};

// ---------------------------------------------------------------------------
// Domain model
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone)]
struct Counter {
    value: i64,
}

/// Commands are plain structs with Serialize for the TypedService contract.
#[derive(serde::Serialize)]
struct Increment {
    amount: i64,
}

#[derive(serde::Serialize)]
struct Decrement {
    amount: i64,
}

impl wee_events::Command for Increment {
    fn command_name(&self) -> wee_events::CommandName {
        wee_events::CommandName::new("counter:increment")
    }
}

impl wee_events::Command for Decrement {
    fn command_name(&self) -> wee_events::CommandName {
        wee_events::CommandName::new("counter:decrement")
    }
}

// ---------------------------------------------------------------------------
// Context
// ---------------------------------------------------------------------------

/// Test context injected by the factory. Carries a configurable bonus value.
#[derive(Default, Clone)]
struct TestContext {
    bonus: i64,
}

impl TestContext {
    fn bonus(&self) -> i64 {
        self.bonus
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn increment(
    ctx: &TestContext,
    entity: &Entity<Counter>,
    cmd: Increment,
) -> crate::Result<Entity<Counter>> {
    Ok(Entity {
        aggregate_id: entity.aggregate_id.clone(),
        revision: entity.revision.clone(),
        state: Counter {
            value: entity.state.value + cmd.amount + ctx.bonus(),
        },
    })
}

async fn decrement(
    ctx: &TestContext,
    entity: &Entity<Counter>,
    cmd: Decrement,
) -> crate::Result<Entity<Counter>> {
    let new_value = entity.state.value - cmd.amount - ctx.bonus();
    if new_value < 0 {
        return Err(Rejection::new("BELOW_ZERO", "counter cannot go below zero").into());
    }
    Ok(Entity {
        aggregate_id: entity.aggregate_id.clone(),
        revision: entity.revision.clone(),
        state: Counter { value: new_value },
    })
}

// ---------------------------------------------------------------------------
// Loader
// ---------------------------------------------------------------------------

async fn load_counter(
    _ctx: &TestContext,
    id: &wee_events::AggregateId,
) -> crate::Result<Entity<Counter>> {
    Ok(Entity {
        aggregate_id: id.clone(),
        revision: Revision::zero(),
        state: Counter::default(),
    })
}

type Result<T> = std::result::Result<T, wee_events::Error>;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn load_returns_default_entity() {
    let service = ServiceBuilder::<Counter>::new()
        .with_loader(load_counter)
        .with_handler::<Increment, _>(increment)
        .build(|| async { Ok(TestContext::default()) });

    let id = AggregateId::new("counter", "test-1");
    let entity = service.load(&id).await.unwrap();

    assert_eq!(entity.state.value, 0);
    assert_eq!(entity.aggregate_id, id);
}

#[tokio::test]
async fn execute_dispatches_to_increment_handler() {
    let service = ServiceBuilder::<Counter>::new()
        .with_loader(load_counter)
        .with_handler::<Increment, _>(increment)
        .build(|| async { Ok(TestContext::default()) });

    let id = AggregateId::new("counter", "test-1");
    let entity = service.execute(&id, Increment { amount: 5 }).await.unwrap();

    assert_eq!(entity.state.value, 5);
}

#[tokio::test]
async fn execute_dispatches_to_correct_handler_among_multiple() {
    let service = ServiceBuilder::<Counter>::new()
        .with_loader(load_counter)
        .with_handler::<Increment, _>(increment)
        .with_handler::<Decrement, _>(decrement)
        .build(|| async { Ok(TestContext::default()) });

    let id = AggregateId::new("counter", "test-1");

    // Increment handler is invoked and returns the updated value.
    let entity = service.execute(&id, Increment { amount: 10 }).await.unwrap();
    assert_eq!(entity.state.value, 10);

    // Decrement handler is invoked.
    // The loader always returns default (value: 0), so we decrement by 0 to stay valid.
    let entity = service.execute(&id, Decrement { amount: 0 }).await.unwrap();
    assert_eq!(entity.state.value, 0);

    // Verify that Decrement's business rule fires correctly (0 - 1 → rejection).
    let err = service
        .execute(&id, Decrement { amount: 1 })
        .await
        .unwrap_err();
    assert!(matches!(err, wee_events::Error::Rejection(r) if r.code == "BELOW_ZERO"));
}

#[tokio::test]
async fn execute_handler_rejection_propagates_as_error() {
    let service = ServiceBuilder::<Counter>::new()
        .with_loader(load_counter)
        .with_handler::<Decrement, _>(decrement)
        .build(|| async { Ok(TestContext::default()) });

    let id = AggregateId::new("counter", "test-1");
    // Decrement from 0 by 5 → BELOW_ZERO rejection
    let err = service
        .execute(&id, Decrement { amount: 5 })
        .await
        .unwrap_err();

    match err {
        wee_events::Error::Rejection(r) => assert_eq!(r.code, "BELOW_ZERO"),
        other => panic!("expected Rejection, got: {other}"),
    }
}

#[tokio::test]
async fn factory_called_once_per_operation() {
    let call_count = Arc::new(Mutex::new(0u32));
    let counter = Arc::clone(&call_count);

    let service = ServiceBuilder::<Counter>::new()
        .with_loader(load_counter)
        .with_handler::<Increment, _>(increment)
        .build(move || {
            let counter = Arc::clone(&counter);
            async move {
                *counter.lock().unwrap() += 1;
                Ok(TestContext::default())
            }
        });

    let id = AggregateId::new("counter", "test-1");

    service.load(&id).await.unwrap();
    assert_eq!(*call_count.lock().unwrap(), 1);

    service.execute(&id, Increment { amount: 1 }).await.unwrap();
    assert_eq!(*call_count.lock().unwrap(), 2);

    service.load(&id).await.unwrap();
    assert_eq!(*call_count.lock().unwrap(), 3);
}

#[tokio::test]
async fn factory_context_bonus_applied_in_handler() {
    let service = ServiceBuilder::<Counter>::new()
        .with_loader(load_counter)
        .with_handler::<Increment, _>(increment)
        .build(|| async {
            Ok(TestContext { bonus: 10 })
        });

    let id = AggregateId::new("counter", "test-1");
    // amount: 2, bonus: 10 → value = 0 + 2 + 10 = 12
    let entity = service.execute(&id, Increment { amount: 2 }).await.unwrap();
    assert_eq!(entity.state.value, 12);
}
