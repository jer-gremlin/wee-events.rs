use crate::ServiceDefinition;

/// A service definition that can build an in-process typed runtime.
///
/// Implemented by the `service!` macro for full service declarations.
pub trait InProcessServiceDefinition: ServiceDefinition {
    type Built<Store, Services>;

    fn build_in_process<Store, Services>(
        store: Store,
        services: Services,
    ) -> Self::Built<Store, Services>;
}

pub struct ServiceCreateBuilder<Service> {
    service: Service,
}

pub struct ServiceCreateStoreBuilder<Service, Store> {
    service: Service,
    store: Store,
}

pub struct ServiceCreateEnvBuilder<Service, Store, Services> {
    service: Service,
    store: Store,
    services: Services,
}

pub fn create<Service>(service: Service) -> ServiceCreateBuilder<Service> {
    ServiceCreateBuilder { service }
}

impl<Service> ServiceCreateBuilder<Service>
where
    Service: InProcessServiceDefinition,
{
    pub fn with_store<Store>(self, store: Store) -> ServiceCreateStoreBuilder<Service, Store> {
        ServiceCreateStoreBuilder {
            service: self.service,
            store,
        }
    }
}

impl<Service, Store> ServiceCreateStoreBuilder<Service, Store>
where
    Service: InProcessServiceDefinition,
{
    pub fn with_env<Services>(
        self,
        services: Services,
    ) -> ServiceCreateEnvBuilder<Service, Store, Services> {
        ServiceCreateEnvBuilder {
            service: self.service,
            store: self.store,
            services,
        }
    }
}

impl<Service, Store, Services> ServiceCreateEnvBuilder<Service, Store, Services>
where
    Service: InProcessServiceDefinition,
{
    pub fn build(self) -> Service::Built<Store, Services> {
        let _ = self.service;
        Service::build_in_process(self.store, self.services)
    }
}
