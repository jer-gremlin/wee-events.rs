use wee_events::{AggregateId, Command, CommandName, Entity, Handles, Revision, TypedService};

#[derive(Debug, Default, Clone)]
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

#[derive(Default, Clone)]
struct TestContext;

async fn load_counter(_ctx: &TestContext, id: &AggregateId) -> wee_events::Result<Entity<Counter>> {
    Ok(Entity {
        aggregate_id: id.clone(),
        revision: Revision::zero(),
        state: Counter::default(),
    })
}

async fn increment(
    _ctx: &TestContext,
    _entity: &Entity<Counter>,
    cmd: Increment,
) -> wee_events::Result<Entity<Counter>> {
    Ok(Entity {
        aggregate_id: "counter:test".parse().unwrap(),
        revision: Revision::zero(),
        state: Counter { value: cmd.amount },
    })
}

async fn adjust(
    _ctx: &TestContext,
    entity: &Entity<Counter>,
    _cmd: Adjust,
) -> wee_events::Result<Entity<Counter>> {
    Ok(entity.clone())
}

wee_events::service! {
    pub CounterService for Counter {
        loader: load_counter,
        handlers: [
            Increment => increment,
            Adjust => adjust,
        ],
    }
}

#[tokio::test]
async fn generated_service_builds_and_executes() {
    let service = CounterService::build(|| async { Ok(TestContext) });
    let id: AggregateId = "counter:c1".parse().unwrap();
    let entity = service.execute(&id, Increment { amount: 3 }).await.unwrap();
    assert_eq!(entity.state.value, 3);
}

#[tokio::test]
async fn generated_service_handles_multiple_commands() {
    let service = CounterService::build(|| async { Ok(TestContext) });
    let id: AggregateId = "counter:c1".parse().unwrap();
    let _ = service.execute(&id, Increment { amount: 3 }).await.unwrap();
    let _ = service.execute(&id, Adjust).await.unwrap();
}

#[tokio::test]
async fn generated_service_loads() {
    let service = CounterService::build(|| async { Ok(TestContext) });
    let id: AggregateId = "counter:c1".parse().unwrap();
    let entity = service.load(&id).await.unwrap();
    assert_eq!(entity.state.value, 0);
}

// ---------------------------------------------------------------------------
// Shared-caller pattern
// ---------------------------------------------------------------------------

/// A function that accepts any `TypedService<Counter>` implementation and
/// exercises both commands. This is the "shared caller" pattern — the same
/// business logic works whether it is given a local service or a remote client.
async fn shared_caller<T>(svc: &T, id: &AggregateId) -> wee_events::Result<Entity<Counter>>
where
    T: TypedService<Counter> + Handles<Increment, ()> + Handles<Adjust, ()>,
{
    let entity = svc.execute(id, Increment { amount: 10 }).await?;
    // Adjust is a no-op in this test implementation; it returns the entity as-is.
    let _ = entity;
    svc.execute(id, Adjust).await
}

/// Verify that the `service!`-generated struct satisfies `TypedService<Counter>`
/// and can be passed to the shared-caller helper.
#[tokio::test]
async fn shared_caller_works_with_service_macro() {
    let service = CounterService::build(|| async { Ok(TestContext) });
    let id: AggregateId = "counter:c1".parse().unwrap();
    let entity = shared_caller(&service, &id).await.unwrap();
    // Adjust returns the entity as loaded (Counter { value: 0 }) since the
    // test loader always returns the default state.
    assert_eq!(entity.state.value, 0);
}
