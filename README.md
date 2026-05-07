# wee-events

`wee-events` is a Rust event-sourcing toolkit with a compact core API, derive
macros for domain modeling, and a libSQL-backed SQLite store for local and
production-friendly persistence.

It is part of the `wee-events` family alongside the original
[`wee-events`](https://github.com/weegigs/wee-events) project and
[`wee-events-go`](https://github.com/weegigs/wee-events-go).

## Features

- **Compact event-sourcing core**: aggregate, event, command, renderer, and store primitives.
- **Typed services**: `TypedService<S>` exposes `load` and compile-time checked `execute` (via `Handles<C>` bounds).
- **Core and Restate service macros**: `service!` declares services once and generates typed in-process and Restate dispatch.
- **Capability-driven handlers**: handlers receive `HandlerEnv<Store, Services>` for store-backed publishing plus service dependencies.
- **Structured rejections**: `Rejection` error type for domain/business logic failures, distinct from infrastructure errors.
- **Typed identifiers**: dedicated types for aggregate IDs, event IDs, revisions, and names.
- **Derive macros**: `Command` and `DomainEvent` derives generate consistent names from enums.
- **In-memory store**: useful for tests and lightweight workflows.
- **SQLite store**: append-only event persistence with document storage and projection helpers.
- **Restate executor**: durable command execution and side-effect dispatch via the Restate SDK.
- **Store conformance tests**: shared test support for event store implementations.

## Typed Service Example

Declare a service once, get compile-time checked `execute` for free:

```rust
use wee_events::service;

service! {
    pub CounterService for Counter {
        loader: load_counter,
        handlers: [
            increment,
            adjust,
        ],
    }
}

let svc = wee_events::create(CounterService)
    .with_store(store)
    .with_env(services)
    .build();
let entity = svc.execute(&id, Increment { amount: 5 }).await?;

// Unregistered commands fail at compile time, not runtime.
// svc.execute(&id, UnknownCmd); // won't compile
```

Generated in-process services and Restate clients implement `TypedService<S>` +
`Handles<C>`, so business logic can be written once and work with either
backend:

```rust
async fn top_up<T>(svc: &T, id: &AggregateId) -> Result<Entity<Counter>>
where
    T: TypedService<Counter> + Handles<Increment>,
{
    svc.execute(id, Increment { amount: 10 }).await
}
```

## Crates

- `crates/wee-events`: core types, traits (`EventStore`, `TypedService`, `Handles`, `HandlerEnv`), renderer, and an in-memory store
- `crates/wee-events-macros`: derive macros for `Command` and `DomainEvent`
- `crates/wee-events-restate`: Restate SDK-based binding, side-effect dispatch, and HTTP client
- `crates/wee-events-sqlite`: a libSQL-backed SQLite event store plus document and projection helpers

```text
crates/
  wee-events/
  wee-events-macros/
  wee-events-restate/
  wee-events-sqlite/
```

## Development

[mise](https://mise.jdx.dev/) is recommended for tool setup.

```sh
mise install
mise exec -- just         # Show available commands
mise exec -- just check   # fmt-check, cargo check, clippy, and tests
mise exec -- just fmt     # Format the workspace
```

If you prefer to run Cargo directly, use the same commands through `mise exec --`.

The `check` recipe runs:

- `cargo fmt --all -- --check`
- `cargo check --workspace --all-targets --all-features`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace --all-features`
