mod commands;
mod counter_publisher;
mod events;
mod randomiser;
mod services;
mod state;
mod system_randomiser;

use std::sync::Arc;

use restate_sdk::prelude::*;
use services::audit_log::{AuditLog, AuditLogImpl};
use services::counter::{CounterService, CounterServiceBinder};
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
    let randomiser = SystemRandomiser;

    HttpServer::new(
        Endpoint::builder()
            .bind(
                wee_events_restate::create(CounterService)
                    .with_store(store)
                    .with_env(randomiser)
                    .serve(),
            )
            .bind(AuditLogImpl.serve())
            .build(),
    )
    .listen_and_serve("0.0.0.0:9080".parse().unwrap())
    .await;
}
