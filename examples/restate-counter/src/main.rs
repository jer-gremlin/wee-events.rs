mod commands;
mod environment;
mod events;
mod randomizer;
mod services;
mod state;

use std::sync::Arc;

use environment::AppServices;
use randomizer::SystemRandomizer;
use restate_sdk::prelude::*;
use services::audit_log::{AuditLog, AuditLogImpl};
use services::counter::{CounterService, CounterServiceBinder};
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
    let services = AppServices::new(store, SystemRandomizer);

    HttpServer::new(
        Endpoint::builder()
            .bind(
                wee_events_restate::create(CounterService)
                    .with_env(services)
                    .serve(),
            )
            .bind(AuditLogImpl.serve())
            .build(),
    )
    .listen_and_serve("0.0.0.0:9080".parse().unwrap())
    .await;
}
