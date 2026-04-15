//! End-to-end tests using testcontainers.
//!
//! These test the **imperative shell** — the full Restate service stack including
//! durable execution, service calls, and side effects. A real Restate container
//! is started via Docker, services are registered, and commands are sent over HTTP.
//!
//! Requires Docker to be running. Skipped if Docker is unavailable.

use std::sync::Arc;

use restate_sdk::prelude::*;
use restate_sdk_testcontainers::TestEnvironment;
use serde_json::json;
use wee_events::memory::MemoryStore;
use wee_events_restate::EntityResponse;

use restate_counter::domain::{build_dispatcher, build_renderer};
use restate_counter::effects::{AuditLogger, LoggingEffect};
use restate_counter::handlers::{
    CounterCommands, CounterComponents, CounterExecutor, CounterLoader, CounterQueries,
};
use restate_counter::random::{RandomGenerator, RandomService};

fn build_endpoint() -> Endpoint {
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

    Endpoint::builder()
        .bind(RandomGenerator.serve())
        .bind(commands.serve())
        .bind(queries.serve())
        .bind(AuditLogger.serve())
        .build()
}

#[tokio::test]
async fn increment_creates_entity_with_positive_value() {
    let env = TestEnvironment::new()
        .start(build_endpoint())
        .await
        .expect("failed to start Restate container — is Docker running?");

    let client = reqwest::Client::new();
    let ingress = env.ingress_url();

    let request = json!({
        "command": {
            "name": "increment",
            "target": { "aggregate_type": "counter", "aggregate_key": "e2e-1" },
            "command": { "amount": 10 }
        },
        "metadata": {
            "correlation_id": "e2e-test-1"
        }
    });

    let resp = client
        .post(format!("{ingress}/counter-executor/run"))
        .header("content-type", "application/json")
        .json(&request)
        .send()
        .await
        .unwrap();

    assert!(resp.status().is_success(), "status: {}", resp.status());

    let body: EntityResponse = resp.json().await.unwrap();
    // Value should be at least 10 (requested amount), possibly more due to random bonus
    assert!(
        body.state["value"].as_i64().unwrap() >= 10,
        "expected value >= 10, got {:?}",
        body.state
    );
}

#[tokio::test]
async fn decrement_rejects_when_balance_is_zero() {
    let env = TestEnvironment::new()
        .start(build_endpoint())
        .await
        .expect("failed to start Restate container — is Docker running?");

    let client = reqwest::Client::new();
    let ingress = env.ingress_url();

    let request = json!({
        "command": {
            "name": "decrement",
            "target": { "aggregate_type": "counter", "aggregate_key": "e2e-empty" },
            "command": { "amount": 5 }
        },
        "metadata": {
            "correlation_id": "e2e-test-2"
        }
    });

    let resp = client
        .post(format!("{ingress}/counter-executor/run"))
        .header("content-type", "application/json")
        .json(&request)
        .send()
        .await
        .unwrap();

    // Should fail — terminal error from the rejection
    assert!(
        resp.status().is_server_error() || resp.status().as_u16() == 400,
        "expected error status, got: {}",
        resp.status()
    );
}

#[tokio::test]
async fn load_returns_current_state() {
    let env = TestEnvironment::new()
        .start(build_endpoint())
        .await
        .expect("failed to start Restate container — is Docker running?");

    let client = reqwest::Client::new();
    let ingress = env.ingress_url();

    // First increment to create some state
    let increment = json!({
        "command": {
            "name": "increment",
            "target": { "aggregate_type": "counter", "aggregate_key": "e2e-load" },
            "command": { "amount": 42 }
        },
        "metadata": {
            "correlation_id": "e2e-test-3a"
        }
    });

    client
        .post(format!("{ingress}/counter-executor/run"))
        .header("content-type", "application/json")
        .json(&increment)
        .send()
        .await
        .unwrap();

    // Now load
    let load_target = json!({
        "aggregate_type": "counter",
        "aggregate_key": "e2e-load"
    });

    let resp = client
        .post(format!("{ingress}/counter-loader/load"))
        .header("content-type", "application/json")
        .json(&load_target)
        .send()
        .await
        .unwrap();

    assert!(resp.status().is_success(), "status: {}", resp.status());

    let body: EntityResponse = resp.json().await.unwrap();
    assert!(
        body.state["value"].as_i64().unwrap() >= 42,
        "expected value >= 42 after increment, got {:?}",
        body.state
    );
}

#[tokio::test]
async fn adjust_modifies_state_nondeterministically() {
    let env = TestEnvironment::new()
        .start(build_endpoint())
        .await
        .expect("failed to start Restate container — is Docker running?");

    let client = reqwest::Client::new();
    let ingress = env.ingress_url();

    // Seed with some value first
    let seed = json!({
        "command": {
            "name": "increment",
            "target": { "aggregate_type": "counter", "aggregate_key": "e2e-adjust" },
            "command": { "amount": 100 }
        },
        "metadata": { "correlation_id": "e2e-adjust-seed" }
    });

    client
        .post(format!("{ingress}/counter-executor/run"))
        .header("content-type", "application/json")
        .json(&seed)
        .send()
        .await
        .unwrap();

    // Now adjust — result depends on the random service
    let adjust = json!({
        "command": {
            "name": "adjust",
            "target": { "aggregate_type": "counter", "aggregate_key": "e2e-adjust" },
            "command": null
        },
        "metadata": { "correlation_id": "e2e-adjust-1" }
    });

    let resp = client
        .post(format!("{ingress}/counter-executor/run"))
        .header("content-type", "application/json")
        .json(&adjust)
        .send()
        .await
        .unwrap();

    assert!(resp.status().is_success(), "status: {}", resp.status());

    let body: EntityResponse = resp.json().await.unwrap();
    // Verify the response contains a valid integer value
    body.state["value"]
        .as_i64()
        .expect("value should be a valid i64");
}
