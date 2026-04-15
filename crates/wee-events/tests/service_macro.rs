use wee_events::{AggregateId, Command, CommandName, Entity, Revision};

#[derive(Debug, Default, Clone)]
struct Counter {
    value: i64,
}

#[derive(Debug, Clone)]
struct Increment {
    amount: i64,
}

impl Command for Increment {
    fn command_name(&self) -> CommandName {
        CommandName::from("counter:increment")
    }
}

#[derive(Debug, Clone)]
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
