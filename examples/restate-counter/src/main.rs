//! Restate Counter Example
//!
//! Demonstrates:
//! - Command handlers with capability-trait context injection
//! - Non-determinism handled durably via a Restate random service
//! - Side-effect workflow triggered after command execution
//!
//! ## Running
//!
//! ```sh
//! cargo run -p restate-counter
//! restate deployments register http://localhost:9080
//!
//! # Increment
//! curl -X POST http://localhost:8080/counter-executor/run \
//!   -H 'content-type: application/json' \
//!   -d '{"command":{"name":"increment","target":{"aggregate_type":"counter","aggregate_key":"c1"},"command":{"amount":5}},"metadata":{"correlation_id":"req-1"}}'
//!
//! # Adjust (fully random)
//! curl -X POST http://localhost:8080/counter-executor/run \
//!   -H 'content-type: application/json' \
//!   -d '{"command":{"name":"adjust","target":{"aggregate_type":"counter","aggregate_key":"c1"},"command":null},"metadata":{"correlation_id":"req-2"}}'
//! ```

mod domain;
mod effects;
mod handlers;
mod random;

use std::sync::Arc;

use restate_sdk::prelude::*;
use wee_events::memory::MemoryStore;

use crate::domain::{build_dispatcher, build_renderer};
use crate::effects::{AuditLogger, LoggingEffect};
use crate::handlers::{
    CounterCommands, CounterComponents, CounterExecutor, CounterLoader, CounterQueries,
};
use crate::random::{RandomGenerator, RandomService};

#[tokio::main]
async fn main() {
    let store = Arc::new(MemoryStore::new());

    let commands = CounterCommands {
        components: CounterComponents {
            dispatcher: build_dispatcher(),
            store: Arc::clone(&store),
            renderer: build_renderer(),
        },
    };
    let queries = CounterQueries {
        components: CounterComponents {
            dispatcher: build_dispatcher(),
            store,
            renderer: build_renderer(),
        },
    };

    let endpoint = Endpoint::builder()
        .bind(RandomGenerator.serve())
        .bind(commands.serve())
        .bind(queries.serve())
        .bind(AuditLogger.serve())
        .build();

    println!("Counter service listening on 0.0.0.0:9080");
    println!("Services:");
    println!("  random            — durable random number generation");
    println!("  counter-executor  — handles commands (uses random service)");
    println!("  counter-loader    — loads state");
    println!("  logging-effect    — side-effect: logs every command");

    HttpServer::new(endpoint)
        .listen_and_serve("0.0.0.0:9080".parse().unwrap())
        .await;
}
