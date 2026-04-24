mod domain;

use domain::{AuditLog, AuditLogImpl, CounterService, CounterServiceBinder, Env};
use restate_sdk::prelude::*;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    HttpServer::new(
        Endpoint::builder()
            .bind(CounterService::restate(|| async { Ok(Env) }).serve())
            .bind(AuditLogImpl.serve())
            .build(),
    )
    .listen_and_serve("0.0.0.0:9080".parse().unwrap())
    .await;
}
