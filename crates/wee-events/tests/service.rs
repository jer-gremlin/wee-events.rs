use std::convert::Infallible;
use std::future::Future;

use serde_json::json;
use wee_events::{
    AggregateId, Command, CommandExecutor, CommandName, Entity, EntityLoader, Handles, Rejection,
    Revision, Service, ServiceError, TypedService,
};

type TestError = ServiceError<Infallible>;

#[test]
fn rejection_displays_code_and_message() {
    let r = Rejection::new("INSUFFICIENT_FUNDS", "balance too low");
    assert_eq!(r.code, "INSUFFICIENT_FUNDS");
    assert_eq!(r.message, "balance too low");
    assert_eq!(r.to_string(), "INSUFFICIENT_FUNDS: balance too low");
}

#[test]
fn rejection_carries_context() {
    let r = Rejection::with_context(
        "LIMIT_EXCEEDED",
        "over the limit",
        serde_json::json!({ "limit": 100, "actual": 150 }),
    );
    assert_eq!(r.context["actual"], 150);
}

#[test]
fn rejection_default_context_is_empty_object() {
    let r = Rejection::new("CODE", "msg");
    assert_eq!(r.context, serde_json::json!({}));
}

#[test]
fn rejection_converts_to_service_error() {
    let r = Rejection::new("CODE", "msg");
    let err: TestError = r.into();
    assert!(matches!(err, ServiceError::Rejection(_)));
}

#[test]
fn rejection_serde_round_trip() {
    let original = Rejection::with_context(
        "LIMIT_EXCEEDED",
        "over the limit",
        serde_json::json!({ "limit": 100 }),
    );
    let json = serde_json::to_string(&original).unwrap();
    let restored: Rejection = serde_json::from_str(&json).unwrap();
    assert_eq!(original, restored);
}

#[test]
fn rejection_deserialize_without_context_defaults_to_empty_object() {
    let json = r#"{"code":"C","message":"m"}"#;
    let r: Rejection = serde_json::from_str(json).unwrap();
    assert_eq!(r.context, serde_json::json!({}));
}

/// A trivial state for testing.
#[derive(Debug, Default, Clone)]
struct Counter {
    value: i64,
}

/// A test implementation that implements both traits.
struct CounterService;

impl EntityLoader<Counter> for CounterService {
    type Error = TestError;

    async fn load(&self, id: &AggregateId) -> Result<Entity<Counter>, Self::Error> {
        Ok(Entity {
            aggregate_id: id.clone(),
            revision: Revision::zero(),
            state: Counter { value: 0 },
        })
    }
}

impl CommandExecutor<Counter> for CounterService {
    type Error = TestError;

    async fn execute(
        &self,
        name: &CommandName,
        target: &AggregateId,
        command: serde_json::Value,
    ) -> Result<Entity<Counter>, Self::Error> {
        let amount = command["amount"].as_i64().unwrap_or(1);
        match name.as_str() {
            "increment" => Ok(Entity {
                aggregate_id: target.clone(),
                revision: Revision::zero(),
                state: Counter { value: amount },
            }),
            _ => Err(Rejection::new("UNKNOWN_COMMAND", format!("unknown: {name}")).into()),
        }
    }
}

#[tokio::test]
async fn entity_loader_loads_state() {
    let svc = CounterService;
    let id: AggregateId = "counter:test-1".parse().unwrap();
    let entity = svc.load(&id).await.unwrap();
    assert_eq!(entity.state.value, 0);
}

#[tokio::test]
async fn command_executor_executes() {
    let svc = CounterService;
    let id: AggregateId = "counter:test-1".parse().unwrap();
    let name = CommandName::from("increment");
    let entity = svc.execute(&name, &id, json!({"amount": 5})).await.unwrap();
    assert_eq!(entity.state.value, 5);
}

#[tokio::test]
async fn command_executor_rejects_unknown() {
    let svc = CounterService;
    let id: AggregateId = "counter:test-1".parse().unwrap();
    let name = CommandName::from("explode");
    let err = svc.execute(&name, &id, json!({})).await.unwrap_err();
    match err {
        ServiceError::Rejection(r) => assert_eq!(r.code, "UNKNOWN_COMMAND"),
        other => panic!("expected Rejection, got: {other}"),
    }
}

/// Verify blanket `Service` impl works — a function accepting `impl Service<Counter>`
/// can call both `load` and `execute`.
async fn use_service<S>(svc: &S) -> Entity<Counter>
where
    S: Service<Counter>,
    <S as EntityLoader<Counter>>::Error: std::fmt::Debug,
    <S as CommandExecutor<Counter>>::Error: std::fmt::Debug,
{
    let id: AggregateId = "counter:svc-1".parse().unwrap();
    let _ = svc.load(&id).await.unwrap();
    let name = CommandName::from("increment");
    svc.execute(&name, &id, json!({"amount": 10}))
        .await
        .unwrap()
}

#[tokio::test]
async fn service_blanket_impl_works() {
    let svc = CounterService;
    let entity = use_service(&svc).await;
    assert_eq!(entity.state.value, 10);
}

// ── Typed service tests ───────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize)]
struct Increment {
    amount: i64,
}

impl Command for Increment {
    const NAME: &'static str = "counter:increment";
}

struct TypedCounterService;

// Implement ServiceState — associates the service with its state type
impl wee_events::__private::ServiceState for TypedCounterService {
    type State = Counter;
}

// Implement DispatchCommand<Increment> — required by Handles<Increment>
impl wee_events::__private::DispatchCommand<Increment> for TypedCounterService {
    type Error = TestError;

    fn dispatch_command(
        &self,
        id: &AggregateId,
        cmd: Increment,
    ) -> impl Future<Output = Result<Entity<Counter>, Self::Error>> + Send {
        let id = id.clone();
        async move {
            Ok(Entity {
                aggregate_id: id,
                revision: Revision::zero(),
                state: Counter { value: cmd.amount },
            })
        }
    }
}

// Handles<Increment> is satisfied because DispatchCommand<Increment> is implemented
impl Handles<Increment> for TypedCounterService {}

impl TypedService<Counter> for TypedCounterService {
    type Error = TestError;

    fn load(
        &self,
        id: &AggregateId,
    ) -> impl Future<Output = Result<Entity<Counter>, Self::Error>> + Send {
        let id = id.clone();
        async move {
            Ok(Entity {
                aggregate_id: id,
                revision: Revision::zero(),
                state: Counter { value: 0 },
            })
        }
    }
    // execute() uses the default impl from TypedService which calls DispatchCommand<C>
}

#[tokio::test]
async fn typed_service_loads_state() {
    let svc = TypedCounterService;
    let id: AggregateId = "counter:test-1".parse().unwrap();
    let entity = svc.load(&id).await.unwrap();
    assert_eq!(entity.state.value, 0);
}

#[tokio::test]
async fn typed_service_executes_registered_command() {
    let svc = TypedCounterService;
    let id: AggregateId = "counter:test-1".parse().unwrap();
    let entity = svc.execute(&id, Increment { amount: 5 }).await.unwrap();
    assert_eq!(entity.state.value, 5);
}
