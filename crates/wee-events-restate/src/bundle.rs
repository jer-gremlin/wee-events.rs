use std::sync::Arc;

use crate::effects::{EffectRouter, EffectTrigger};
use crate::error::Error;
use crate::executor::CommandHandler;
use crate::loader::LoadHandler;
use crate::service::ServiceAdapter;

/// Bundles all Restate components for a service.
pub struct ServiceBundle<T> {
    pub name: String,
    pub executor: CommandHandler<T>,
    pub loader: LoadHandler<T>,
    pub effect_router: EffectRouter,
}

impl<T> ServiceBundle<T> {
    /// Returns the names of all registered side-effect workflows.
    pub fn effect_names(&self) -> Vec<&str> {
        self.effect_router
            .effects
            .iter()
            .map(|e| e.workflow_name.as_str())
            .collect()
    }
}

/// Creates a new service bundle from a concrete service implementation.
///
/// The service must implement `EntityLoader<S> + CommandExecutor<S>` for
/// some state type `S` that is `Default + Serialize + Send + Sync`.
pub fn service_bundle<S, T>(
    name: impl Into<String>,
    service: T,
    effects: Vec<EffectTrigger>,
) -> ServiceBundle<ServiceAdapter<S, T>>
where
    S: Default + serde::Serialize + Send + Sync + 'static,
    T: wee_events::EntityLoader<S> + wee_events::CommandExecutor<S> + 'static,
    <T as wee_events::EntityLoader<S>>::Error: Into<Error>,
    <T as wee_events::CommandExecutor<S>>::Error: Into<Error>,
{
    let name = name.into();
    let service = Arc::new(ServiceAdapter::<S, T>::new(service));

    ServiceBundle {
        executor: CommandHandler::new(&name, Arc::clone(&service)),
        loader: LoadHandler::new(&name, Arc::clone(&service)),
        effect_router: EffectRouter::new(&name, effects),
        name,
    }
}
