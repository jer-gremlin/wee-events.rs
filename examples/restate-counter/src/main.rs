mod commands;
mod events;
mod randomiser;
mod services;
mod state;

use std::sync::Arc;

use randomiser::system::SystemRandomiser;
use restate_sdk::prelude::*;
use services::audit_log::ConsoleAuditLog;
use services::counter::CounterService;
use wee_events_sqlite::{GlobalStrategy, SqliteEventStore};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let store = Arc::new(
        SqliteEventStore::builder()
            .local("counter-events.db")
            .strategy(GlobalStrategy)
            .writer(wee_events::JsonEncoder)
            .open()
            .await
            .expect("event store should open"),
    );
    let randomiser = SystemRandomiser;

    HttpServer::new(
        wee_events_restate::create(CounterService)
            .with_store(store)
            .with_env(randomiser)
            .with_effect(ConsoleAuditLog)
            .attach_to(Endpoint::builder())
            .build(),
    )
    .listen_and_serve("0.0.0.0:9080".parse().unwrap())
    .await;
}
