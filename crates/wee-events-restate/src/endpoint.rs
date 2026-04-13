use std::sync::Arc;

use crate::effects::{EffectRunner, SideEffect};
use crate::executor::Executor;
use crate::loader::Loader;
use crate::service::{ErasedService, ServiceAdapter};

pub struct ServiceBundle {
    pub name: String,
    pub executor: Executor,
    pub loader: Loader,
    pub effect_runner: EffectRunner,
}

impl ServiceBundle {
    pub fn new<S, T>(
        name: impl Into<String>,
        service: T,
        effects: Vec<SideEffect>,
    ) -> Self
    where
        S: Default + serde::Serialize + Send + Sync + 'static,
        T: wee_events::EntityLoader<S> + wee_events::CommandExecutor<S> + 'static,
    {
        let name = name.into();
        let erased: Arc<dyn ErasedService> = Arc::new(ServiceAdapter::<S, T>::new(service));

        Self {
            executor: Executor::new(&name, Arc::clone(&erased)),
            loader: Loader::new(&name, Arc::clone(&erased)),
            effect_runner: EffectRunner::new(&name, effects),
            name,
        }
    }

    pub fn effect_names(&self) -> Vec<&str> {
        self.effect_runner
            .effects
            .iter()
            .map(|e| e.workflow_name.as_str())
            .collect()
    }
}
