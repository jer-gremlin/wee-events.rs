mod commands;
mod counter_environment;
mod counter_repository;
mod events;
mod randomizer;
mod services;
mod sqlite_counter_repository;
mod state;
mod system_randomizer;

use std::sync::Arc;

use counter_environment::CounterEnvironment;
use restate_sdk::prelude::*;
use services::audit_log::{AuditLog, AuditLogImpl};
use services::counter::{CounterService, CounterServiceBinder};
use sqlite_counter_repository::SqliteCounterRepository;
use system_randomizer::SystemRandomizer;
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
    let repository = SqliteCounterRepository::new(store);
    let environment = CounterEnvironment::new(repository, SystemRandomizer);

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
