use serde::Deserialize;
use serde_json::json;
use wee_events::{
    memory::MemoryStore, AggregateId, CommandExecutor, CommandName, Dispatcher, DomainService,
    Entity, EntityLoader, EventData, EventType, RawEvent, Rejection, Renderer, Service,
};

#[derive(Debug, Default, Clone)]
struct Counter {
    value: i64,
}

#[derive(Debug, Deserialize)]
struct Increment {
    amount: i64,
}

#[derive(Debug, Deserialize)]
struct Decrement {
    amount: i64,
}

struct CounterCtx;

async fn increment<Ctx>(
    _ctx: &Ctx,
    _entity: &Entity<Counter>,
    cmd: Increment,
) -> Result<Vec<RawEvent>, Rejection> {
    Ok(vec![RawEvent {
        event_type: EventType::from("counter:incremented"),
        data: EventData::json(&json!({ "amount": cmd.amount })).unwrap(),
    }])
}

async fn decrement<Ctx>(
    _ctx: &Ctx,
    entity: &Entity<Counter>,
    cmd: Decrement,
) -> Result<Vec<RawEvent>, Rejection> {
    if cmd.amount > entity.state.value {
        return Err(Rejection::new("INSUFFICIENT_VALUE", "cannot go below zero"));
    }
    Ok(vec![RawEvent {
        event_type: EventType::from("counter:decremented"),
        data: EventData::json(&json!({ "amount": cmd.amount })).unwrap(),
    }])
}

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

fn build_service() -> DomainService<CounterCtx, Counter, MemoryStore> {
    let dispatcher = Dispatcher::new()
        .handler("increment", increment)
        .handler("decrement", decrement);

    let renderer = Renderer::new()
        .with("counter:incremented", reduce_incremented)
        .with("counter:decremented", reduce_decremented);

    DomainService::new(CounterCtx, dispatcher, MemoryStore::new(), renderer)
}

#[tokio::test]
async fn load_uninitialized_entity() {
    let svc = build_service();
    let id: AggregateId = "counter:test".parse().unwrap();
    let entity = svc.load(&id).await.unwrap();
    assert_eq!(entity.state.value, 0);
    assert!(!entity.initialized());
}

#[tokio::test]
async fn execute_command_updates_state() {
    let svc = build_service();
    let id: AggregateId = "counter:test".parse().unwrap();

    let entity = svc
        .execute(&CommandName::from("increment"), &id, json!({ "amount": 5 }))
        .await
        .unwrap();

    assert_eq!(entity.state.value, 5);
    assert!(entity.initialized());
}

#[tokio::test]
async fn multiple_commands_accumulate() {
    let svc = build_service();
    let id: AggregateId = "counter:test".parse().unwrap();

    svc.execute(&CommandName::from("increment"), &id, json!({ "amount": 3 }))
        .await
        .unwrap();

    let entity = svc
        .execute(&CommandName::from("increment"), &id, json!({ "amount": 7 }))
        .await
        .unwrap();

    assert_eq!(entity.state.value, 10);
}

#[tokio::test]
async fn handler_rejection_propagates() {
    let svc = build_service();
    let id: AggregateId = "counter:test".parse().unwrap();

    let err = svc
        .execute(
            &CommandName::from("decrement"),
            &id,
            json!({ "amount": 100 }),
        )
        .await
        .unwrap_err();

    match err {
        wee_events::Error::Rejection(r) => assert_eq!(r.code, "INSUFFICIENT_VALUE"),
        other => panic!("expected Rejection, got: {other}"),
    }
}

#[tokio::test]
async fn unknown_command_rejected() {
    let svc = build_service();
    let id: AggregateId = "counter:test".parse().unwrap();

    let err = svc
        .execute(&CommandName::from("explode"), &id, json!({}))
        .await
        .unwrap_err();

    match err {
        wee_events::Error::Rejection(r) => assert_eq!(r.code, "HANDLER_NOT_FOUND"),
        other => panic!("expected Rejection, got: {other}"),
    }
}

#[tokio::test]
async fn service_blanket_impl_works() {
    let svc = build_service();

    async fn use_service(svc: &impl Service<Counter>) {
        let id: AggregateId = "counter:svc".parse().unwrap();
        let entity = svc
            .execute(&CommandName::from("increment"), &id, json!({ "amount": 1 }))
            .await
            .unwrap();
        assert_eq!(entity.state.value, 1);
    }

    use_service(&svc).await;
}
