mod bundle;
mod client;
mod correlation;
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
pub use correlation::correlation_id;
pub use effects::{EffectRouter, EffectTrigger, SideEffectFilter};
pub use executor::CommandHandler;
pub use loader::LoadHandler;
pub use names::{executor_name, loader_name, runner_name};
pub use service::{JsonService, ServiceAdapter, ServiceResponse};
pub use types::{CommandRequest, EntityResponse, ExecuteNotification, ExecuteRequest, Metadata};

/// Hidden re-exports and helpers consumed by macro-generated code.
#[doc(hidden)]
pub mod __private {
    use crate::types::EntityResponse;

    pub use restate_sdk::context::Context;
    pub use restate_sdk::{context, errors, object, serde};

    pub fn to_handler_error(e: wee_events::Error) -> restate_sdk::errors::HandlerError {
        match e {
            wee_events::Error::Rejection(r) => {
                let payload = serde_json::json!({
                    "code": r.code,
                    "message": r.message,
                    "context": r.context,
                });
                restate_sdk::errors::TerminalError::new(payload.to_string()).into()
            }
            e => e.into(),
        }
    }

    pub fn to_entity_response<S: ::serde::Serialize>(
        entity: wee_events::Entity<S>,
    ) -> Result<EntityResponse, restate_sdk::errors::HandlerError> {
        Ok(EntityResponse {
            aggregate: entity.aggregate_id,
            revision: entity.revision,
            state: serde_json::to_value(&entity.state)
                .map_err(|e| restate_sdk::errors::TerminalError::new(e.to_string()))?,
        })
    }
}
