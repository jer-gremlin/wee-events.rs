use serde::Deserialize;
use serde_json::json;
use wee_events::{AggregateId, CommandName, Dispatcher, Entity, EventData, Rejection, Revision};

#[derive(Debug, Deserialize)]
struct Increment {
    amount: i64,
}

#[derive(Debug, Deserialize)]
struct Reset;

#[derive(Debug, Deserialize)]
struct Decrement {
    amount: i64,
}

#[derive(Debug, Default, Clone)]
struct Counter {
    value: i64,
}

trait HasMultiplier {
    fn multiplier(&self) -> i64;
}

struct TestCtx {
    multiplier: i64,
}

impl HasMultiplier for TestCtx {
    fn multiplier(&self) -> i64 {
        self.multiplier
    }
}

async fn increment_handler<Ctx: HasMultiplier>(
    ctx: &Ctx,
    _entity: &Entity<Counter>,
    cmd: Increment,
) -> Result<Vec<wee_events::RawEvent>, Rejection> {
    let amount = cmd.amount * ctx.multiplier();
    Ok(vec![wee_events::RawEvent {
        event_type: wee_events::EventType::from("counter:incremented"),
        data: EventData::json(&json!({ "amount": amount })).unwrap(),
    }])
}

async fn decrement_handler<Ctx>(
    _ctx: &Ctx,
    entity: &Entity<Counter>,
    cmd: Decrement,
) -> Result<Vec<wee_events::RawEvent>, Rejection> {
    if cmd.amount > entity.state.value {
        return Err(Rejection::new("INSUFFICIENT_VALUE", "cannot go below zero"));
    }
    Ok(vec![wee_events::RawEvent {
        event_type: wee_events::EventType::from("counter:decremented"),
        data: EventData::json(&json!({ "amount": cmd.amount })).unwrap(),
    }])
}

async fn reset_handler<Ctx>(
    _ctx: &Ctx,
    _entity: &Entity<Counter>,
    _cmd: Reset,
) -> Result<Vec<wee_events::RawEvent>, Rejection> {
    Ok(vec![wee_events::RawEvent {
        event_type: wee_events::EventType::from("counter:reset"),
        data: EventData::json(&json!({})).unwrap(),
    }])
}

fn test_entity() -> Entity<Counter> {
    Entity {
        aggregate_id: AggregateId::new("counter", "1"),
        revision: Revision::zero(),
        state: Counter { value: 42 },
    }
}

#[tokio::test]
async fn dispatch_routes_to_correct_handler() {
    let dispatcher = Dispatcher::<TestCtx, Counter>::new()
        .handler("increment", increment_handler)
        .handler("reset", reset_handler);

    let ctx = TestCtx { multiplier: 2 };
    let entity = test_entity();

    let events = dispatcher
        .dispatch(
            &ctx,
            &entity,
            &CommandName::from("increment"),
            json!({ "amount": 5 }),
        )
        .await
        .unwrap();

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_type.as_str(), "counter:incremented");
    let data: serde_json::Value = events[0].data.deserialize_json().unwrap();
    assert_eq!(data["amount"], 10); // 5 * 2
}

#[tokio::test]
async fn dispatch_rejects_unknown_command() {
    let dispatcher = Dispatcher::<TestCtx, Counter>::new().handler("increment", increment_handler);

    let ctx = TestCtx { multiplier: 1 };
    let entity = test_entity();

    let err = dispatcher
        .dispatch(&ctx, &entity, &CommandName::from("explode"), json!({}))
        .await
        .unwrap_err();

    assert_eq!(err.code, "HANDLER_NOT_FOUND");
}

#[tokio::test]
async fn dispatch_rejects_invalid_command_payload() {
    let dispatcher = Dispatcher::<TestCtx, Counter>::new().handler("increment", increment_handler);

    let ctx = TestCtx { multiplier: 1 };
    let entity = test_entity();

    let err = dispatcher
        .dispatch(
            &ctx,
            &entity,
            &CommandName::from("increment"),
            json!("not an object"),
        )
        .await
        .unwrap_err();

    assert_eq!(err.code, "COMMAND_VALIDATION_ERROR");
}

#[tokio::test]
async fn dispatch_handler_with_no_context_requirements() {
    let dispatcher = Dispatcher::<TestCtx, Counter>::new().handler("reset", reset_handler);

    let ctx = TestCtx { multiplier: 1 };
    let entity = test_entity();

    let events = dispatcher
        .dispatch(&ctx, &entity, &CommandName::from("reset"), json!(null))
        .await
        .unwrap();

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_type.as_str(), "counter:reset");
}

#[tokio::test]
async fn dispatch_handler_reads_entity_state() {
    let dispatcher = Dispatcher::<TestCtx, Counter>::new().handler("decrement", decrement_handler);

    let ctx = TestCtx { multiplier: 1 };
    let entity = Entity {
        aggregate_id: AggregateId::new("counter", "1"),
        revision: Revision::zero(),
        state: Counter { value: 10 },
    };

    // Decrement within bounds succeeds
    let events = dispatcher
        .dispatch(
            &ctx,
            &entity,
            &CommandName::from("decrement"),
            json!({ "amount": 5 }),
        )
        .await
        .unwrap();
    assert_eq!(events.len(), 1);

    // Decrement exceeding value is rejected
    let err = dispatcher
        .dispatch(
            &ctx,
            &entity,
            &CommandName::from("decrement"),
            json!({ "amount": 100 }),
        )
        .await
        .unwrap_err();
    assert_eq!(err.code, "INSUFFICIENT_VALUE");
}
