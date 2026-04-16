mod aggregate;
mod command;
mod dispatcher;
mod domain_service;
mod entity;
mod error;
mod event;
mod id;
mod memory_store;
mod renderer;
mod service;
mod service_builder;
mod spec;
mod store;

#[cfg(any(test, feature = "testing"))]
mod bench_suite;
#[cfg(any(test, feature = "testing"))]
mod test_suite;

pub use aggregate::Aggregate;
pub use command::Command;
pub use dispatcher::Dispatcher;
pub use domain_service::DomainService;
pub use entity::Entity;
pub use error::{Error, RetryDiagnostics};
pub use event::{ChangeSet, DomainEvent, EventData, EventMetadata, RecordedEvent};
pub use id::{
    AggregateId, AggregateIdParseError, AggregateType, CommandName, CorrelationId, EventId,
    EventType, Revision,
};
pub use renderer::{ReduceFn, Renderer};
pub use service::{CommandExecutor, EntityLoader, Handles, Rejection, Service, TypedService};
pub use service_builder::{BuiltService, ServiceBuilder};
#[doc(hidden)]
pub use service_builder::{
    FactoryBridge, HandleCommand, HandlerBridge, HandlerList, LoaderBridge,
};
#[doc(hidden)]
pub use service_builder::{EmptyHandlers, Here, There};
#[doc(hidden)]
pub use service::__private;
pub use store::{EventStore, PublishOptions, RawEvent};
pub use spec::{HandlerSpec, LoaderSpec};
pub use wee_events_macros::{handler, loader, service, Command, DomainEvent};

pub mod memory {
    pub use crate::memory_store::{MemoryStore, MemoryStoreBacking};
}

#[cfg(any(test, feature = "testing"))]
pub mod testing {
    pub use crate::bench_suite::*;
    pub use crate::shared_store_test_suite;
    pub use crate::store_bench_suite;
    pub use crate::store_test_suite;
    pub use crate::test_suite::*;
}

pub type Result<T> = std::result::Result<T, Error>;

/// Helper to serialize a domain event into a `RawEvent` with JSON encoding.
pub fn to_raw_event<E: DomainEvent + serde::Serialize>(event: &E) -> Result<RawEvent> {
    Ok(RawEvent {
        event_type: event.event_type(),
        data: EventData::json(event)?,
    })
}
