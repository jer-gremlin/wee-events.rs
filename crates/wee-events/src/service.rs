use crate::command::Command;
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

/// Hidden implementation details used by generated code.
#[doc(hidden)]
pub mod __private {
    use super::*;
    use core::future::Future;

    /// Marker trait that associates a service with its state type via an
    /// associated type.
    ///
    /// Using an associated type rather than a type parameter keeps the public
    /// `Handles<C>` trait single-parameter. The impl
    /// `impl ServiceState for PubService { type State = PrivateState; }` is
    /// always valid regardless of `PrivateState`'s visibility — associated type
    /// values in impl blocks may reference private types.
    pub trait ServiceState: Send + Sync {
        type State;
    }

    /// Carries the compile-time dispatch witness for command `C`.
    ///
    /// `S` is recovered through `Self::State` from the `ServiceState` supertrait,
    /// so callers only need one type parameter. Implemented per concrete command
    /// by the `service!` macro with the specific `Idx` type computed from the
    /// handler registration order.
    pub trait DispatchCommand<C>: ServiceState + Send + Sync {
        fn dispatch_command(
            &self,
            id: &AggregateId,
            cmd: C,
        ) -> impl Future<Output = crate::Result<Entity<Self::State>>> + Send;
    }
}

/// Marker supertrait declaring that a service can handle command type `C`.
///
/// The `__private::DispatchCommand<C>` supertrait carries the actual dispatch
/// implementation. Callers only see `Handles<C>` — a single type parameter.
///
/// Implement via the `service!` macro (which generates the concrete
/// `DispatchCommand<C>` impl) or manually by implementing both
/// `ServiceState`, `DispatchCommand<C>`, and `Handles<C>`.
pub trait Handles<C>: __private::DispatchCommand<C> {}

/// A typed service contract combining state loading with type-safe command dispatch.
///
/// Unlike `Service<S>` (which takes untyped JSON), `TypedService<S>` dispatches
/// over concrete command types. The `Handles<C>` bound on `execute` ensures
/// only registered commands can be dispatched — unregistered commands produce a
/// compile error rather than a runtime rejection.
///
/// The trait is transport-agnostic: no `Serialize`, `Deserialize`, or
/// transport-specific bounds appear here. Serialization is an adapter concern
/// handled by generated macro code.
pub trait TypedService<S>: __private::ServiceState<State = S> + Send + Sync {
    fn load(
        &self,
        id: &AggregateId,
    ) -> impl core::future::Future<Output = crate::Result<Entity<S>>> + Send;

    fn execute<C>(
        &self,
        id: &AggregateId,
        cmd: C,
    ) -> impl core::future::Future<Output = crate::Result<Entity<S>>> + Send
    where
        C: crate::Command + Send + 'static,
        Self: Handles<C>,
    {
        __private::DispatchCommand::<C>::dispatch_command(self, id, cmd)
    }
}

/// Defines the identity and state type of a service.
///
/// This trait links a service marker type to its aggregate state and a human-readable
/// service name. It is the minimal contract for service definitions.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a service definition",
    note = "use the `service!` macro to generate a ServiceDefinition impl",
)]
pub trait ServiceDefinition: Send + Sync + 'static {
    type State;
    const SERVICE_NAME: &'static str;
}

/// Declares that a service can handle command type `C`.
///
/// Used to statically assert that a command has been declared in a service.
/// Typically implemented automatically by the `service!` macro when a command
/// is registered in the command list.
#[diagnostic::on_unimplemented(
    message = "`{Self}` does not declare command `{C}`",
    label = "add `{C}` to the `service!` command list",
    note = "commands must be listed in the service! declaration for routing"
)]
pub trait HasCommand<C: Command>: Send + Sync {}
