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

pub trait RestateServiceDefinition {
    type Binding<Store, Services>;

    fn bind<Store, Services>(store: Store, services: Services) -> Self::Binding<Store, Services>;
}

pub struct RestateServiceBuilder<Service> {
    service: Service,
}

pub struct RestateServiceStoreBuilder<Service, Store> {
    service: Service,
    store: Store,
}

pub struct HandlerEnv<Store, Services> {
    store: Store,
    services: Services,
}

impl<Store, Services> HandlerEnv<Store, Services> {
    pub fn new(store: Store, services: Services) -> Self {
        Self { store, services }
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn services(&self) -> &Services {
        &self.services
    }
}

impl<Store, Services> wee_events::HasPublisher for HandlerEnv<Store, Services>
where
    Store: wee_events::EventStore,
    Services: Send + Sync,
{
    type Store = Store;

    fn publisher(&self) -> wee_events::Publisher<'_, Self::Store> {
        wee_events::Publisher::new(self.store())
    }
}

pub fn create<Service>(service: Service) -> RestateServiceBuilder<Service> {
    RestateServiceBuilder { service }
}

impl<Service> RestateServiceBuilder<Service>
where
    Service: RestateServiceDefinition,
{
    pub fn with_store<Store>(self, store: Store) -> RestateServiceStoreBuilder<Service, Store> {
        RestateServiceStoreBuilder {
            service: self.service,
            store,
        }
    }

    pub fn with_env<Services>(self, services: Services) -> Service::Binding<Services, Services>
    where
        Services: Clone,
    {
        let _ = self.service;
        Service::bind(services.clone(), services)
    }
}

impl<Service, Store> RestateServiceStoreBuilder<Service, Store>
where
    Service: RestateServiceDefinition,
{
    pub fn with_env<Services>(self, services: Services) -> Service::Binding<Store, Services> {
        let _ = self.service;
        Service::bind(self.store, services)
    }
}

/// Hidden re-exports and helpers consumed by macro-generated code.
#[doc(hidden)]
pub mod __private {
    use crate::types::EntityResponse;

    pub use restate_sdk::context::Context;
    pub use restate_sdk::{context, errors, object, serde};
    pub use serde_json;

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
