mod commands;
mod counter_environment;
mod counter_loader;
mod counter_publisher;
mod events;
mod randomiser;
mod services;
mod sqlite_counter_loader;
mod sqlite_counter_publisher;
mod state;
mod system_randomiser;

use std::sync::Arc;

use counter_environment::CounterEnvironment;
use restate_sdk::prelude::*;
use services::audit_log::{AuditLog, AuditLogImpl};
use services::counter::{CounterService, CounterServiceBinder};
use sqlite_counter_loader::SqliteCounterLoader;
use sqlite_counter_publisher::SqliteCounterPublisher;
use system_randomiser::SystemRandomiser;
use wee_events_sqlite::{GlobalStrategy, SqliteEventStore};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let store = Arc::new(
        SqliteEventStore::builder()
            .local("counter-events.db")
            .strategy(GlobalStrategy)
            .open()
            .await
            .expect("event store should open"),
    );
    let loader = SqliteCounterLoader::new(Arc::clone(&store));
    let publisher = SqliteCounterPublisher::new(store);
    let randomiser = SystemRandomiser;
    let environment = CounterEnvironment::new(loader, publisher, randomiser);

    HttpServer::new(
        Endpoint::builder()
            .bind(
                wee_events_restate::create(CounterService)
                    .with_env(environment)
                    .serve(),
            )
            .bind(AuditLogImpl.serve())
            .build(),
    )
    .listen_and_serve("0.0.0.0:9080".parse().unwrap())
    .await;
}
