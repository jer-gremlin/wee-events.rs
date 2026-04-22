//! Adapter-specific capability traits for the Restate runtime.
//!
//! These traits carry Restate-only capabilities that handlers can opt into
//! via `#[handler(..., requires(HasRestateContext))]`. Handlers that require
//! such capabilities are bound to the Restate runtime by construction — they
//! cannot be satisfied by the portable interpreter, and that asymmetry is a
//! feature: attempting to run a Restate-only handler under the portable
//! runtime is a compile error.

use restate_sdk::context::Context;

/// Capability exposing the Restate durable-execution context.
///
/// Implemented by the per-request environment type that the Restate server
/// binder constructs. Handlers that call durable primitives such as
/// `ctx.run(...)`, `ctx.service_client::<_>()`, or the durable RNG declare
/// `requires(HasRestateContext)` and take the context via this trait.
pub trait HasRestateContext {
    fn restate_context(&self) -> &Context<'_>;
}
