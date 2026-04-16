//! Compile-time metadata for service handlers and loaders.
//!
//! These traits are emitted by the `#[handler]` and `#[loader]` attribute
//! macros as a companion to each annotated function. The `service!` macro
//! consumes them by deterministic naming convention to synthesize the
//! service-specific environment contract.

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

/// Metadata for a state loader function.
///
/// Emitted by `#[wee_events::loader(...)]` as a zero-size companion struct.
pub trait LoaderSpec: 'static {
    /// The state type the loader produces.
    type State;
}
