//! End-to-end test for the generated `CounterService::restate(factory)`
//! constructor and the `RestateDispatch` trait.
//!
//! - Compile-time: the constructor produces a value that implements
//!   `RestateDispatch`.
//! - Runtime (no live Restate context): the dispatch trait object is reachable
//!   from the value the macro emits, matching the end-to-end shape an adapter
//!   binder will consume.

use serde::{Deserialize, Serialize};
use wee_events::{AggregateId, Command, Entity, Revision};
use wee_events_restate::RestateDispatch;

// ---------------------------------------------------------------------------
// Domain — state and commands (Serialize + Deserialize for transport)
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Counter {
    value: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Increment {
    amount: i64,
}

impl Command for Increment {
    const NAME: &'static str = "counter:increment";
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Adjust;

impl Command for Adjust {
    const NAME: &'static str = "counter:adjust";
}

// ---------------------------------------------------------------------------
// Capability trait — simple, no Restate coupling for this test.
// ---------------------------------------------------------------------------

trait HasClock: Send + Sync + 'static {
    fn now(&self) -> i64;
}

// ---------------------------------------------------------------------------
// Loader + handlers — generic over R: HasClock
// ---------------------------------------------------------------------------

#[wee_events::loader(requires(HasClock))]
async fn load_counter<R: HasClock>(
    _env: &R,
    id: &AggregateId,
) -> wee_events::Result<Entity<Counter>> {
    Ok(Entity {
        aggregate_id: id.clone(),
        revision: Revision::zero(),
        state: Counter::default(),
    })
}

#[wee_events::handler(command = Increment, requires(HasClock))]
async fn increment<R: HasClock>(
    env: &R,
    entity: &Entity<Counter>,
    cmd: Increment,
) -> wee_events::Result<Entity<Counter>> {
    Ok(Entity {
        aggregate_id: entity.aggregate_id.clone(),
        revision: entity.revision.clone(),
        state: Counter {
            value: entity.state.value + cmd.amount + env.now(),
        },
    })
}

#[wee_events::handler(command = Adjust, requires(HasClock))]
async fn adjust<R: HasClock>(
    _env: &R,
    entity: &Entity<Counter>,
    _cmd: Adjust,
) -> wee_events::Result<Entity<Counter>> {
    Ok(entity.clone())
}

// ---------------------------------------------------------------------------
// Service declaration — full form, emits portable + restate constructors.
// ---------------------------------------------------------------------------

wee_events::service! {
    pub CounterService("counter") for Counter {
        loader: load_counter,
        handlers: [increment, adjust],
    }
}

// ---------------------------------------------------------------------------
// Test env — satisfies CounterServiceEnv via the blanket impl on HasClock.
// ---------------------------------------------------------------------------

#[derive(Default)]
struct TestEnv;

impl HasClock for TestEnv {
    fn now(&self) -> i64 {
        42
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Compile-time witness that `CounterService::restate(factory)` produces a
/// value implementing `RestateDispatch`.
#[test]
fn restate_factory_produces_dispatch() {
    fn assert_dispatch<T: RestateDispatch>(_: &T) {}
    let server = CounterService::restate(|_ctx| async { Ok(TestEnv) });
    assert_dispatch(&server);
}

// ---------------------------------------------------------------------------
// Runtime: exercise the name-first HList dispatch directly.
//
// The full `RestateDispatch::execute` path requires a live `restate_sdk`
// `Context<'_>`, which has no public constructor outside of a running Restate
// service. The genuinely new routing code is `HandleByName` — the
// blanket-impl wrapper is trivial glue. Test `HandleByName` directly.
// ---------------------------------------------------------------------------

use wee_events::{CommandName, ServiceBuilder};
use wee_events_restate::HandleByName;

fn counter_handlers() -> impl HandleByName<TestEnv, Counter> {
    let builder = ServiceBuilder::<Counter>::new()
        .with_loader(load_counter::<TestEnv>)
        .with_handler::<Increment, _>(increment::<TestEnv>)
        .with_handler::<Adjust, _>(adjust::<TestEnv>);
    // The returned `BuiltService` has `.handlers` with exactly the type we
    // need to exercise name-first dispatch.
    let built = builder.build(|| async { Ok(TestEnv) });
    built.handlers
}

#[tokio::test]
async fn handle_by_name_dispatches_matching_command() {
    let handlers = counter_handlers();
    let env = TestEnv;
    let entity = Entity {
        aggregate_id: "counter:c1".parse().unwrap(),
        revision: Revision::zero(),
        state: Counter { value: 3 },
    };
    let name: CommandName = "counter:increment".into();
    let payload = serde_json::json!({ "amount": 5 });

    let result = handlers
        .handle_by_name(&env, &entity, &name, payload)
        .await
        .unwrap();

    let updated = result.expect("matching command should return Some");
    // original (3) + cmd.amount (5) + env.now() (42) = 50
    assert_eq!(updated.state.value, 50);
}

#[tokio::test]
async fn handle_by_name_returns_none_for_unknown_command() {
    let handlers = counter_handlers();
    let env = TestEnv;
    let entity = Entity {
        aggregate_id: "counter:c1".parse().unwrap(),
        revision: Revision::zero(),
        state: Counter::default(),
    };
    let name: CommandName = "counter:unknown".into();

    let result = handlers
        .handle_by_name(&env, &entity, &name, serde_json::Value::Null)
        .await
        .unwrap();

    assert!(
        result.is_none(),
        "unknown command name must produce None so the caller can emit a clean rejection"
    );
}
