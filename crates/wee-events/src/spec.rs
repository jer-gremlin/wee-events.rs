//! Compile-time metadata for service handlers and loaders.
//!
//! These traits are emitted by the `#[handler]` and `#[loader]` attribute
//! macros as a companion to each annotated function. The `service!` macro
//! consumes them by deterministic naming convention to synthesize the
//! service-specific environment contract.

#[doc(hidden)]
pub type HandlerFuture<'a, S, E> = std::pin::Pin<
    Box<
        dyn std::future::Future<Output = Result<crate::service_builder::HandlerOutcome<S>, E>>
            + Send
            + 'a,
    >,
>;

#[doc(hidden)]
pub type LoaderFuture<'a, S, E> =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<crate::Entity<S>, E>> + Send + 'a>>;

/// Metadata for a command handler function.
///
/// Emitted by `#[wee_events::handler(...)]` as a zero-size companion struct.
/// The `service!` macro uses these associated types to generate the
/// service's environment trait and dispatch wiring.
pub trait HandlerSpec: 'static {
    /// The concrete command type this handler accepts.
    type Command: crate::Command;
    /// The state type the handler produces/updates.
    type State;
}

/// Runtime metadata for a command handler against a concrete environment.
///
/// Emitted by `#[wee_events::handler(...)]` alongside [`HandlerSpec`].
pub trait HandlerRuntimeSpec<R>: HandlerSpec {
    /// The error type returned when the handler is called with environment `R`.
    type Error;

    #[doc(hidden)]
    fn handle<'a>(
        env: &'a R,
        entity: &'a crate::Entity<Self::State>,
        command: Self::Command,
    ) -> HandlerFuture<'a, Self::State, Self::Error>
    where
        R: 'a,
        Self::State: 'a,
        Self::Error: 'a,
        Self::Command: 'a;
}

/// Metadata for a state loader function.
///
/// Emitted by `#[wee_events::loader(...)]` as a zero-size companion struct.
pub trait LoaderSpec: 'static {
    /// The state type the loader produces.
    type State;
}

/// Runtime metadata for a loader against a concrete environment.
///
/// Emitted by `#[wee_events::loader(...)]` alongside [`LoaderSpec`].
pub trait LoaderRuntimeSpec<R>: LoaderSpec {
    /// The error type returned when the loader is called with environment `R`.
    type Error;

    #[doc(hidden)]
    fn load<'a>(
        env: &'a R,
        id: &'a crate::AggregateId,
    ) -> LoaderFuture<'a, Self::State, Self::Error>
    where
        R: 'a,
        Self::State: 'a,
        Self::Error: 'a;
}
