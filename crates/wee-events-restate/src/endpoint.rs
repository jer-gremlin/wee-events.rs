use std::sync::Arc;

use crate::effects::{EffectRunner, SideEffect};
use crate::executor::Executor;
use crate::loader::Loader;
use crate::service::ServiceAdapter;

/// Bundles all Restate components for a service.
pub struct ServiceBundle<T> {
    pub name: String,
    pub executor: Executor<T>,
    pub loader: Loader<T>,
    pub effect_runner: EffectRunner,
}

impl<T> ServiceBundle<T> {
    /// Returns the names of all registered side-effect workflows.
    pub fn effect_names(&self) -> Vec<&str> {
        self.effect_runner
            .effects
            .iter()
            .map(|e| e.workflow_name.as_str())
            .collect()
    }
}

impl ServiceBundle<()> {
    /// Creates a new service bundle from a concrete service implementation.
    ///
    /// The service must implement `EntityLoader<S> + CommandExecutor<S>` for
    /// some state type `S` that is `Default + Serialize + Send + Sync`.
    pub fn new<S, T>(
        name: impl Into<String>,
        service: T,
        effects: Vec<SideEffect>,
    ) -> ServiceBundle<ServiceAdapter<S, T>>
    where
        S: Default + serde::Serialize + Send + Sync + 'static,
        T: wee_events::EntityLoader<S> + wee_events::CommandExecutor<S> + 'static,
    {
        let name = name.into();
        let service = Arc::new(ServiceAdapter::<S, T>::new(service));

        ServiceBundle {
            executor: Executor::new(&name, Arc::clone(&service)),
            loader: Loader::new(&name, Arc::clone(&service)),
            effect_runner: EffectRunner::new(&name, effects),
            name,
        }
    }
}
