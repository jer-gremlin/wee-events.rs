use crate::entity::Entity;
use crate::id::{AggregateId, CommandName};

/// A structured rejection from the domain layer. Indicates a command was
/// refused by business logic (as opposed to an infrastructure failure).
///
/// Carries a machine-readable code, human message, and arbitrary JSON context.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, thiserror::Error)]
#[error("{code}: {message}")]
pub struct Rejection {
    pub code: String,
    pub message: String,
    #[serde(default = "default_context")]
    pub context: serde_json::Value,
}

fn default_context() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

impl Rejection {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            context: default_context(),
        }
    }

    pub fn with_context(
        code: impl Into<String>,
        message: impl Into<String>,
        context: serde_json::Value,
    ) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            context,
        }
    }
}

/// Loads projected entity state for an aggregate.
///
/// This is the "read" half of a service — given an aggregate ID, return
/// the current projected state. Implementations typically load from an
/// EventStore and render through a Renderer.
#[allow(async_fn_in_trait)]
pub trait EntityLoader<S>: Send + Sync {
    async fn load(&self, id: &AggregateId) -> crate::Result<Entity<S>>;
}

/// Executes a named command against a target aggregate, returning the
/// updated projected state.
///
/// Takes an untyped `serde_json::Value` payload — this is the
/// service-boundary interface. Typed command dispatch (validation,
/// deserialization into concrete command enums) happens inside
/// implementations.
///
/// Returns `crate::Result` so implementations can propagate both
/// infrastructure errors (store failures, serialization) and domain
/// rejections (`Error::Rejection`). Callers distinguish the two by
/// pattern-matching on the `Error` enum.
#[allow(async_fn_in_trait)]
pub trait CommandExecutor<S>: Send + Sync {
    async fn execute(
        &self,
        name: &CommandName,
        target: &AggregateId,
        command: serde_json::Value,
    ) -> crate::Result<Entity<S>>;
}

/// A service combines entity loading with command execution.
///
/// Blanket-implemented for any type that implements both `EntityLoader<S>`
/// and `CommandExecutor<S>`.
pub trait Service<S>: EntityLoader<S> + CommandExecutor<S> {}
impl<S, T: EntityLoader<S> + CommandExecutor<S>> Service<S> for T {}

/// Marker trait that declares a service can handle command type `C`.
///
/// The `Idx` type parameter encodes the handler's position in a type-level
/// handler list (using `Here` / `There<Idx>` selector types from
/// `service_builder`). This avoids overlapping-impl errors when multiple
/// commands are registered. The default `Idx = ()` keeps the trait ergonomic
/// for hand-written single-command impls.
///
/// Implement this for each command type your service supports. The
/// `TypedService::execute` method requires `Self: Handles<C, Idx>` so the
/// compiler rejects calls with unregistered command types at compile time.
pub trait Handles<C, Idx = ()> {}

/// A typed service contract combining state loading with type-safe command dispatch.
///
/// Unlike `Service<S>` (which takes untyped JSON), `TypedService<S>` dispatches
/// over concrete command types. The `Handles<C, Idx>` bound on `execute` ensures
/// only registered commands can be dispatched — unregistered commands produce a
/// compile error rather than a runtime rejection.
///
/// The `Idx` type parameter is inferred by the compiler; callers do not need
/// to specify it.
pub trait TypedService<S>: Send + Sync {
    fn load(
        &self,
        id: &AggregateId,
    ) -> impl core::future::Future<Output = crate::Result<Entity<S>>> + Send;

    fn execute<C, Idx>(
        &self,
        id: &AggregateId,
        cmd: C,
    ) -> impl core::future::Future<Output = crate::Result<Entity<S>>> + Send
    where
        C: crate::Command + serde::Serialize + Send + 'static,
        Self: Handles<C, Idx>;
}
