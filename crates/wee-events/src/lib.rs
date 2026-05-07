mod aggregate;
mod codec;
mod command;
mod dispatcher;
mod domain_service;
mod entity;
mod error;
mod event;
mod id;
mod memory_store;
mod publisher;
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
pub use codec::{
    CborDecoder, CborEncoder, CodecError, DecodeError, DecoderList, EncodeError, EncodesEvents,
    EventDecoder, EventDecoders, EventEncoder, JsonDecoder, JsonEncoder,
};
pub use command::Command;
pub use dispatcher::Dispatcher;
pub use domain_service::DomainService;
pub use entity::Entity;
pub use error::{Error, EventStoreErrorExt, RetryDiagnostics};
pub use event::{
    ChangeSet, DeserializeJsonError, DomainEvent, EventData, EventMetadata, RecordedEvent,
};
pub use id::{
    AggregateId, AggregateIdParseError, AggregateType, CommandName, CorrelationId, EventId,
    EventType, Revision,
};
pub use publisher::{HasPublisher, Publisher};
pub use renderer::{ReduceFn, Renderer};
#[doc(hidden)]
pub use service::__private;
pub use service::{
    CommandExecutor, EntityLoader, Handles, HasCommand, Rejection, Service, ServiceDefinition,
    ServiceError, TypedService,
};
pub use service_builder::{BuiltService, ServiceBuilder};
#[doc(hidden)]
pub use service_builder::{EmptyHandlers, Here, There};
#[doc(hidden)]
pub use service_builder::{FactoryBridge, HandleCommand, HandlerBridge, HandlerList, LoaderBridge};
#[doc(hidden)]
pub use service_builder::{HandlerOutcome, IntoHandlerOutcome};
pub use spec::{HandlerSpec, LoaderSpec};
pub use store::{EventStore, PublishOptions, RawEvent};
pub use wee_events_macros::{capability, handler, loader, service, Command, DomainEvent};

pub mod memory {
    pub use crate::memory_store::{MemoryStore, MemoryStoreBacking, MemoryStoreError};
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
pub fn to_raw_event<E: DomainEvent + serde::Serialize>(
    event: &E,
) -> std::result::Result<RawEvent, EncodeError> {
    Ok(RawEvent {
        event_type: event.event_type(),
        data: JsonEncoder.serialize(event)?,
    })
}
