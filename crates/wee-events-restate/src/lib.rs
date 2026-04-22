mod bundle;
mod client;
mod context;
mod dispatch;
mod effects;
mod executor;
#[doc(hidden)]
pub mod generated;
mod loader;
#[doc(hidden)]
pub mod names;
mod service;
mod types;

pub use bundle::{service_bundle, ServiceBundle};
pub use client::RestateClient;
pub use context::HasRestateContext;
pub use dispatch::RestateDispatch;
#[doc(hidden)]
pub use dispatch::{HandleByName, RestateFactoryBridge};
pub use effects::{EffectRouter, EffectTrigger, SideEffectFilter};
pub use executor::CommandHandler;
pub use loader::LoadHandler;
pub use names::{executor_name, loader_name, runner_name};
pub use service::{JsonService, ServiceAdapter, ServiceResponse};
pub use types::{CommandRequest, EntityResponse, ExecuteNotification, ExecuteRequest, Metadata};

/// Hidden re-exports consumed by macro-generated code.
///
/// Referenced by the `service!` macro's `restate` constructor so the generated
/// bound on the factory closure can name `restate_sdk::context::Context`
/// without forcing downstream crates to depend on `restate-sdk` directly.
#[doc(hidden)]
pub mod __private {
    pub use restate_sdk::context::Context;
}
