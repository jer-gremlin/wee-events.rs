# wee-events.rs — Review

# TODO

## Core crate — `crates/wee-events/`

### Renderer

- [ ] **[ARCH]** `src/renderer.rs:10-11` — `ReduceFn<S, E> = fn(&mut S, ...)` is a function pointer. Forbids closures and stateful reducers.
	> SuGestion: `Box<dyn Fn(...) -> Result<...> + Send + Sync>` — one indirection, unblocks captures.
- [ ] **[ARCH]** `src/renderer.rs:245-274` — Hand-rolled `*`-glob matcher with no visible tests.
	> SuGestion: depend on `globset`, or admit it's prefix-match and use `str::starts_with`.
- [ ] **[FOOTGUN]** `src/renderer.rs:117-120` — No way to inspect a `Renderer` after construction. Write-only.
	> SuGestion: add `fn covers(&self, event_type: &EventType) -> bool` so coverage can be asserted from tests.
- [ ] **[FOOTGUN]** `examples/restate-counter/src/state.rs:14-19` — `Renderer` wired by hand against `CounterEvent::INCREMENTED` etc. Add a `DomainEvent` variant, forget to wire it → compile clean, runtime `UnhandledEventType`.
	> SuGestion: a `renderer!(EnumName, State, { Variant => fn, ... })` macro that emits a `match self` body so rustc's exhaustiveness lint catches missed variants; or rewrite `Renderer::render` to take a typed `fn(&mut S, E)` and do the runtime `RecordedEvent → E` decode inside, letting `match` enforce exhaustiveness in user code.

### service_builder / service

- [ ] **[FOOTGUN]** `src/service_builder.rs:64-79` — `HandlerOutcome::Reload` means "handler returned `()`, the framework reloads from the store." Every ergonomic `Ok(())` silently issues a second `load`.
	> SuGestion: require handlers to return the new `Entity<S>` explicitly, or document the second-load as load-bearing perf cost.
- [ ] **[BUG]** `src/service_builder.rs:276-290` — Two `unsafe impl Send` / `unsafe impl Sync` for `BuiltService`. Inner data is `F`/`L`/`Handlers` (already `Send + Sync`) plus `PhantomData<fn() -> _>` (auto-`Send + Sync`). Auto-derived impls would be identical and safe.
	> SuGestion: delete both `unsafe impl` blocks; if the build still passes, they were dead. If it fails, find and fix the offending field rather than re-asserting the unsafe.
- [ ] **[FOOTGUN]** `src/service_builder.rs:266-274` — `BuiltService` fields all `pub` including `_ctx`/`_state`/`_loader_error`/`_handler_error`.
	> SuGestion: `pub(crate)` if macro-internal; document if genuinely public.
- [ ] **[BUG]** `src/service_builder.rs:299-300` — `EH: From<crate::Error> + From<EL>` + `EL: From<crate::Error>` means two distinct conversion paths `crate::Error → EH` exist. If they produce different values, dispatch is non-deterministic.
	> SuGestion: pick one path explicitly, or seal the conversion via a single `Into` impl.
- [ ] **[ARCH]** `src/service_builder.rs:453-475` — `build_raw` (`#[doc(hidden)]`) is a near-duplicate of `build` that skips the trait bound. Adapters can call it to bypass safety.
	> SuGestion: gate behind a feature flag or delete; backdoors that are technically public aren't private.
- [ ] **[ARCH]** `src/service.rs:51-93` — `ServiceError` has five `From` impls. Same source reaches the type via two paths. Asymmetric: `From<RenderError<DecodeError>>` exists, `From<RenderError<DeserializeJsonError>>` doesn't.
	> SuGestion: one canonical error type with a single `Custom` variant; delete the `From` cycles.
- [ ] **[ARCH]** `src/service.rs:127-160` + `:170` — `Handles<C>` → `DispatchCommand<C>` → `ServiceState` trait sandwich. `#[diagnostic::on_unimplemented]` is on the wrong traits — users see `Handles<C>` in mismatch errors but the diagnostic lives on `ServiceDefinition` / `HasCommand<C>`.
	> SuGestion: move the diagnostic to `Handles<C>`; collapse the sandwich if possible.
- [ ] **[ARCH]** `src/service.rs:197-218` — `TypedService<S>` has redundant `S` type parameter recoverable via `ServiceState<State = S>`.
	> SuGestion: drop `<S>`, infer everywhere.
- [ ] **[FOOTGUN]** `src/handler_env.rs:39-61` — `HandlerEnv: EventStore` blanket impl. Handlers can publish to any aggregate, skipping `Publisher::publish`'s revision guard. (See `crates/sins/src/bin/handler_env_bypasses_publisher.rs` for proof.)
	> SuGestion: drop the blanket impl; handlers reach the store via `env.store()` so the unusual path is grep-able. For cross-aggregate writes, an explicit `&CrossAggregateStore` capability.

### Identity & data

- [ ] **[ARCH]** `src/id.rs:13-50` (PARTIAL — `Cow` migration done; `Revision`, `AggregateId.aggregate_key`, and `AggregateType` lifted out of the macro and backed by `Arc<str>` for cheap clone) — `newtype_id!` still emits `PartialOrd, Ord` on `EventType`/`CommandName` where ordering is meaningless, and three near-identical entry points (`new`, `From<String>`, `From<&str>`). `EventType`/`CommandName`/`CorrelationId` still `Cow`-backed; switching them to `Arc<str>` would lose `new_const` (derive-macro-emitted) so deferred.
	> SuGestion: drop the `Ord`/`PartialOrd` derives on discriminator types; pick one constructor entry point.
- [ ] **[FOOTGUN]** `src/id.rs:111-167` — `AggregateId::new("", "")` is accepted; `Display`/`FromStr` round-trip silently re-partitions inputs with embedded `:`. (See `crates/sins/src/bin/aggregate_id_no_roundtrip.rs`.)
	> SuGestion: validate at construction (reject empty / colon-containing type fragments) or change the wire separator to something neither side legally contains.
- [ ] **[FOOTGUN]** `src/id.rs:224-238` — `EventType`/`CommandName`/`AggregateType` accept any string despite "kebab-case by convention" doc. (See `crates/sins/src/bin/newtype_id_validates_nothing.rs`.)
	> SuGestion: `TryFrom<&str>` enforces the convention; `new_const` keeps the no-alloc path for static literals.
- [ ] **[ARCH]** `src/event.rs:138-141` — `DomainEvent: Serialize + Deserialize + Send + 'static`. Emission only needs `Serialize`.
	> SuGestion: split `EmitsEvent` / `DecodesEvent`.

### Misc core

- [ ] **[ARCH]** `src/lib.rs:24-56` — 60+ re-exports at the crate root, half `#[doc(hidden)]`. Macro-internal types (`Here`, `There`, `HandlerBridge`, `HandlerOutcome`, `__private`, ...) leak into the user-facing namespace.
	> SuGestion: move all macro-internal types to one `pub mod __macro_support` module; stop using `#[doc(hidden)]` as a privacy modifier.
- [ ] **[NIT]** `src/lib.rs:60` — `pub mod memory` with one type inside is needless namespacing.
	> SuGestion: re-export at crate root.
- [ ] **[NIT]** `src/lib.rs:75-82` — `to_raw_event` free function duplicates one branch of `Publisher::publish`.
	> SuGestion: make it a method on `RawEvent` or move next to its sole caller.
- [ ] **[FOOTGUN]** `src/memory_store.rs:21` — `Ulid(Box<dyn std::error::Error + Send + Sync>)` boxes a concrete `MonotonicError`.
	> SuGestion: type the variant: `Ulid(ulid::MonotonicError)`.
- [ ] **[BUG]** `src/memory_store.rs:35-39` — `From<serde_json::Error>` always wraps as `EncodeError::Json`, even for decode failures. (See `crates/sins/src/bin/memory_store_decode_labelled_encode.rs`.)
	> SuGestion: don't have a blanket `From` for ambiguous-direction errors; require the call site to pick `Encode` or `Decode` explicitly.
- [ ] **[NIT]** `src/memory_store.rs` — Multiple `expect("...mutex poisoned")` calls on the same mutexes.
	> SuGestion: wrap in a small `poisoned!` helper.
- [ ] **[BUG]** `src/entity.rs:14-16` — `Entity::initialized() = !revision.is_zero()`. Empty-string revisions report initialized. (See `crates/sins/src/bin/entity_initialised_lies.rs`.)
	> SuGestion: compute from event count or hold a `bool initialized` field set explicitly on successful first-load.
- [ ] **[NIT]** `src/aggregate.rs:8` vs `:39-46` — `pub id: AggregateId` field but `events`/`revision` private with accessors. Inconsistent.
	> SuGestion: pick one — all `pub` or all accessor-only.
- [ ] **[ARCH]** `src/create.rs:1-70` — Three-step typestate builder. Intermediate structs carry a `service: Service` field discarded at line 67. The builder is a no-op around a static function call.
	> SuGestion: delete the builder; expose `Service::new(store, services)` directly.
- [ ] **[ARCH]** `src/spec.rs:9-19` + `service_builder.rs` — Two boxed-future type aliases (`HandlerFuture`, `LoaderFuture`, `BoxFuture`) for the same shape.
	> SuGestion: one alias in one place.
- [ ] **[ARCH]** `src/spec.rs:26-77` — Four traits purely for macro consumption (`HandlerSpec`, `HandlerRuntimeSpec`, `LoaderSpec`, `LoaderRuntimeSpec`) at the crate root.
	> SuGestion: move to `pub mod __macro_support`.
- [ ] **[ARCH]** `src/publisher.rs:55-59` — `HasPublisher` trait with one impl.
	> SuGestion: delete or document why the abstraction.
- [ ] **[ARCH]** `src/bench_suite.rs` — 600-line bench scaffolding bakes in sharding-model assumptions (`TypeStrategy`, `make_spread_id`, `make_concentrated_id`) that belong in `wee-events-sqlite`.
	> SuGestion: move sharding-specific bench code to `wee-events-sqlite/benches`. Keep the generic store-trait benches here.

## SQLite crate — `crates/wee-events-sqlite/`

### Concurrency / correctness

- [ ] **[FOOTGUN]** `src/event_store/store.rs:126-164` + `:250` — `enumerate_aggregates` `try_join_all`s over every known partition unbounded.
	> SuGestion: bound concurrency via `buffered(N)`; document the cost; recommend a different strategy for enumeration.
- [ ] **[NIT]** `src/event_store/store.rs:267-275` — `sorted_unique_ids` clones + sorts + dedups O(n log n).
	> SuGestion: `HashSet<AggregateId>`.

### Partitioning machinery

- [ ] **[ARCH]** `src/event_store/strategies/*.rs` — Five strategies with near-identical scaffolding; three traits + two marker traits.
	> SuGestion: collapse to one `PartitionStrategy` trait; delete the marker traits.
- [ ] **[FOOTGUN]** `src/event_store/strategies/by_aggregate.rs:64-97` + `by_type.rs:61-90` — `partition_from_target_name` runs `SELECT DISTINCT` to recover identity; full table scan per discovery.
	> SuGestion: rely on `_wee_events_partition_metadata`; drop the fallback.
- [ ] **[ARCH]** `src/event_store/partitioning.rs` — One trait whose every backend delegates to a provisioner. With `BackendBinding`, three layers for one job.
	> SuGestion: fold catalog into provisioner.
- [ ] **[FOOTGUN]** `src/event_store/backends/remote.rs:82-115` — `NamedTargetCatalog::partitions` opens a libsql connection per discovered target.
	> SuGestion: cache.
- [ ] **[ARCH]** `src/event_store/types.rs:54-126` — Three marker subtraits with no methods exist only to gate builder methods.
	> SuGestion: delete the markers.

### Turso adapter

- [ ] **[ARCH]** `src/event_store/turso_platform/` (1329 lines) — Hand-rolled HTTP client, name sanitizer, metadata sidecar, infra-mutating `cleanup()`. All in the core SQLite crate.
	> SuGestion: extract to `wee-events-turso` crate.
- [ ] **[BUG]** `src/event_store/turso_platform/mod.rs:122-123, :281` — `cache: Mutex<HashMap>` and `known_names: Mutex<HashSet>` must stay consistent and don't.
	> SuGestion: one source of truth.
- [ ] **[BUG]** `src/event_store/turso_platform/mod.rs:296-314` — `create_database` `AlreadyExists` path unwraps `Some` from `get_database`; TOCTOU against the platform's eventual consistency.
	> SuGestion: retry/backoff.

### Persistence layer

- [ ] **[BUG]** `src/database.rs:150-184` — Migration's version-write happens outside the BEGIN block; failure mid-migration leaves schema upgraded but unrecorded. Works today only because every DDL is `IF NOT EXISTS`.
	> SuGestion: bundle DDL + version write into one transaction.
- [ ] **[FOOTGUN]** `src/database.rs:171` — `format!("BEGIN;{ddl}")` interpolates DDL strings. Safe today (constants only); invites injection later.
	> SuGestion: `tx.execute_batch`.

### Error & ergonomics

- [ ] **[ARCH]** `src/error.rs:1-20` — `Configuration(String)` / `Internal(String)` dumping grounds; Turso `ApiError` variants stringified then string-matched back in `is_lazy_create_partition_not_ready`.
	> SuGestion: typed `Provisioner(Box<dyn Error>)` / `Api(ApiError)` variants.
- [ ] **[FOOTGUN]** `src/document_store.rs:19-21` — Single `Mutex<Connection>` for all collections + keys.
	> SuGestion: document as test-only, or pool connections.
- [ ] **[BUG]** `src/document_store.rs:51-61` — `upsert` accepts any revision string. A non-ULID lex-smaller revision silently wins the comparison.
	> SuGestion: type the comparison; reject non-ULID revisions at the boundary.
- [ ] **[BUG]** `src/projections.rs:39-72` — `rebuild_projection` enumerates all aggregate IDs into a `Vec`; unbounded memory, no checkpointing. Concurrent writers between `load` and `upsert` cause lost projections.
	> SuGestion: paginate; re-check tail aggregates after rebuild.
- [ ] **[ARCH]** `src/projections.rs:7-30` — `apply_projection` is hard-coded to the `SqliteEventStore` default alias.
	> SuGestion: make it generic.

### Public surface & tests

- [ ] **[ARCH]** `src/lib.rs:1-20` — ~30 names re-exported flat.
	> SuGestion: group into `strategy::*` / `partition::*` / `store::*` modules.
- [ ] **[FOOTGUN]** `Cargo.toml:11` + `dev-dependencies` — `turso_platform::sanitize` compiles regardless of feature; `testcontainers` is an unconditional dev-dep.
	> SuGestion: gate behind an integration feature.
- [ ] **[FOOTGUN]** `Cargo.toml:15` — `libsql = "0.9.29"` against an unstable 0.x.z crate.
	> SuGestion: pin more aggressively + CI smoke.
- [ ] **[FOOTGUN]** `tests/conformance.rs:30-152` — `optional_store_test_suite!` early-exits each test when sqld is missing; CI sees green tests that ran zero assertions.
	> SuGestion: one runner test, or `#[ignore]` semantics.
- [ ] **[FOOTGUN]** `tests/conformance.rs:659, :723` — `Box::leak(container)` deliberately leaks testcontainers.
	> SuGestion: `tokio::sync::OnceCell` with explicit teardown phase.
- [ ] **[NIT]** `tests/conformance.rs:34-184` — Huge macro that calls `testing::foo(&store).await` per case.
	> SuGestion: one `run_suite(store)` function.
- [ ] **[BUG]** `tests/multi_process_local.rs` — Misnamed: spins up two stores in **one process**.
	> SuGestion: rename, or shell out to a second binary.
- [ ] **[NIT]** `tests/documents.rs:71-94` — Hand-rolled temp-file cleanup via Drop, leaks on panic.
	> SuGestion: `tempfile::tempdir()`.

## Macros — `crates/wee-events-macros/`

- [ ] **[BUG]** `src/lib.rs` + `handler_attr.rs`, `loader_attr.rs`, `capability_attr.rs` — Derive macros emit `wee_events::...` without leading `::`.
	> SuGestion: rewrite uniformly to `::wee_events::...`.
- [ ] **[ARCH]** `src/lib.rs:200-225` — `extract_prefix` overloads parameter `attr_name` as both attribute identifier and suffix-strip key.
	> SuGestion: split into two functions.
- [ ] **[BUG]** `src/lib.rs:222` — Suffix stripping is conditional; doc says "suffix stripped" without qualification.
	> SuGestion: fix the doc.
- [ ] **[ARCH]** `src/handler_attr.rs:74-256` — Massive copy-paste; same 12-word error string 6× verbatim.
	> SuGestion: extract `unwrap_result<N>(ty) -> [Type; N]`.
- [ ] **[BUG]** `src/handler_attr.rs:184, 214` — Silent fallback to `wee_events::Error` when type inference fails.
	> SuGestion: require explicit `Result<T, E>` or refuse.
- [ ] **[BUG]** `src/handler_attr.rs:273-279` — "At least one generic type parameter" check doesn't verify the first param is the context arg type.
	> SuGestion: parse the signature properly.
- [ ] **[FOOTGUN]** `src/handler_attr.rs:300, 303` — `{fn_name}_Spec` ident collisions across modules sharing scope.
	> SuGestion: hygienic ident via `proc_macro2::Span::mixed_site`.
- [ ] **[ARCH]** `src/loader_attr.rs:1-323` — Clone of `handler_attr.rs` minus the command.
	> SuGestion: share helpers.
- [ ] **[BUG]** `src/capability_attr.rs:35-43` — Rewrites every fn output uniformly into `impl Future + Send`; sync methods silently become async.
	> SuGestion: distinguish.
- [ ] **[FOOTGUN]** `src/capability_attr.rs:53-58` — Receiver restrictions undocumented; error message misleading.
	> SuGestion: document; improve the message.
- [ ] **[NIT]** `src/service_macro.rs:253-269` — Reinvents `to_snake_case`; `convert_case` is already a dep.
	> SuGestion: use the existing dep.
- [ ] **[BUG]** `src/service_macro.rs:277-298` — `spec_path` appends `_Spec` to the last segment; re-exports break this.
	> SuGestion: emit `$path::Spec` (an associated type), or use full-crate paths.
- [ ] **[ARCH]** `src/service_macro.rs:700-708` — `const _: () = { ... }` "assertion" exists "to silence the unused import."
	> SuGestion: delete.
- [ ] **[FOOTGUN]** `src/service_macro.rs:611` — Generated mod uses `use super::*;`.
	> SuGestion: import specifics.
- [ ] **[FOOTGUN]** `src/service_macro.rs:825-836` — `__wee_events_handler_0` idents show up in OpenAPI/Restate logs as `handler_3 failed`.
	> SuGestion: use the source ident.
- [ ] **[ARCH]** `src/service_macro.rs:976-1027` — Type-state ladder generates O(2^n) blanket impls.
	> SuGestion: cap at 4 effects, or rewrite using a single-impl approach.
- [ ] **[FOOTGUN]** `src/service_macro.rs:807-819` — `Clone + Serialize` added to every command when any effect is declared.
	> SuGestion: bound only the commands actually filtered.
- [ ] **[FOOTGUN]** `src/service_macro.rs:884-902` — Effect-side JSON serialization failure becomes a `TerminalError` mid-execution.
	> SuGestion: pre-flight serialize before entering the side-effect closure; surface differently.
- [ ] **[BUG]** `src/service_macro.rs:341, 457` etc. — `/// Service definition for #name.` literal-token in doc comment; never expands.
	> SuGestion: `#[doc = format!(...)]`.
- [ ] **[BUG]** `crates/wee-events-macros/tests/` — Doesn't exist. No `trybuild` UI tests.
	> SuGestion: add.

## Restate adapter — `crates/wee-events-restate/`

- [ ] **[BUG]** `src/names.rs:1-11` vs `src/service_macro.rs` — Client constructs `"counter-side-effect-executor"`, server registers as `"counter"` (raw). No e2e test forces them to agree.
	> SuGestion: write the e2e test; fix whichever side is wrong.
- [ ] **[ARCH]** `src/client.rs:30-41` — `executor_name` / `encode_key` / `generate_correlation_id` exist as both private methods and free functions.
	> SuGestion: one location.
- [ ] **[FOOTGUN]** `src/client.rs:48-107` — `execute_idempotent` unreachable via `TypedService`.
	> SuGestion: expose through the trait or delete.
- [ ] **[BUG]** `src/client.rs:82-97` — Error-body parsing falls back to `Backend(text)` with full response body.
	> SuGestion: cap size; preserve status code.
- [ ] **[FOOTGUN]** `src/lib.rs:80-87` — `RestateServiceBuilder::with_env` (no store path) uses env as store.
	> SuGestion: delete; force callers to pass the store explicitly.
- [ ] **[ARCH]** `src/lib.rs:21-39` — `Ready` and `Needs<R, T>` undocumented.
	> SuGestion: docstrings explaining the typestate.
- [ ] **[ARCH]** `src/lib.rs:100-180` — `pub mod __private` re-exports half of `restate_sdk`.
	> SuGestion: lock to only what generated code needs.
- [ ] **[NIT]** `src/lib.rs:104` — `pub use restate_sdk::context::Context;` is dead.
	> SuGestion: drop.
- [ ] **[ARCH]** `src/effects.rs:24-69` — `EffectRouter` / `EffectTrigger` exist; nothing uses them.
	> SuGestion: delete.
- [ ] **[ARCH]** `src/types.rs:37-76` — Hand-written serde impls for `EntityResponse` + `ExecuteNotification` with identical bodies.
	> SuGestion: one `Json<T>` wrapper.
- [ ] **[ARCH]** `src/error.rs:53-60` — `From<ServiceError<E>>` collapses `Store(E)` into `Backend(e.to_string())`.
	> SuGestion: separate `Store` variant.
- [ ] **[NIT]** `src/correlation.rs:13` — Format joins `:`-bearing parts with `-`. Parser-unfriendly.
	> SuGestion: unambiguous separator.
- [ ] **[ARCH]** `src/lib.rs:101-179` — `IntoHandlerError` barely used; only implemented for two types.
	> SuGestion: blanket via `Display`, or remove.

## Workspace / build / docs

- [ ] **[FOOTGUN]** `Cargo.toml:10` — `exclude = ["examples/coroutine-door"]` undocumented.
	> SuGestion: comment explaining why; add to README.
- [ ] **[ARCH]** Workspace dep coverage — `bytes`, `nanoid`, `reqwest`, `restate-sdk`, `ulid` not in `[workspace.dependencies]`; `restate-sdk` separately pinned in the example.
	> SuGestion: hoist into the workspace section.
- [ ] **[ARCH]** `Cargo.toml:24` — `tokio = { features = ["full"] }` workspace-wide.
	> SuGestion: minimal features per crate.
- [ ] **[BUG]** `README.md:42-46` — Code sample uses `wee_events::create(...)` which isn't the real API.
	> SuGestion: fix the sample to match what the macro emits.
- [ ] **[NIT]** `README.md:8-9` — No explanation of what the Rust port is, why it exists, what's stable.
	> SuGestion: write a one-paragraph status.
- [ ] **[NIT]** `README.md:22` — Single sentence for the Restate executor surface.
	> SuGestion: link the `restate-counter` example; show one snippet.
- [ ] **[BUG]** `README.md` + `wee-events-macros/Cargo.toml` — `TODO: set public repository/homepage/documentation URLs`. No stability disclaimer despite `0.1.0`.
	> SuGestion: stability disclaimer; set URLs or remove the TODO.
- [ ] **[BUG]** `justfile:4` — `check: fmt fmt-check ...`. `fmt` mutates files **before** `fmt-check` runs, so the check always passes.
	> SuGestion: split `check` (read-only: `fmt-check`, `cargo-check`, `clippy`, `test`) from `fix` (mutating: `fmt`).
- [ ] **[FOOTGUN]** `justfile:1-19` — No `--locked`, no `nextest`, no `doc` recipe.
	> SuGestion: add.
- [ ] **[BUG]** `.github/` — Absent. No CI enforces `just check`.
	> SuGestion: at minimum a `cargo check --locked --workspace --all-features` + `cargo test --locked` on PR.

## Benchmarks — `crates/wee-events/benches/` + `src/bench_suite.rs`

### Criterion config

- [ ] **[METHOD]** All groups — `Throughput::Elements`/`Bytes` never set.
	> SuGestion: `group.throughput(Throughput::Elements(n as u64))`.

### Runtime overhead

- [ ] **[METHOD]** `bench_suite.rs:566` — `Runtime::new()` defaults to multi-thread. For an in-memory store, work-stealing wakeups dominate.
	> SuGestion: run on `current_thread` *and* `multi_thread`, report delta.
- [ ] **[METHOD]** `b.to_async(rt).iter(...)` — Per-iteration task spawn.
	> SuGestion: `iter_custom` with one `block_on` for sub-µs ops.

### Realism / coverage

- [ ] **[GAP]** `bench_suite.rs:42-53` — Sharding claims unsupported: no bench varies `Strategy`.
- [ ] **[GAP]** No payload-size axis (every event is the same small struct).
- [ ] **[GAP]** No "one aggregate, many real publishers" bench.
- [ ] **[GAP]** No "many aggregates, one publisher" sustained-throughput bench.
- [ ] **[GAP]** No cache-hit vs cache-miss `load`.
- [ ] **[GAP]** No p99 / tail-latency.
- [ ] **[GAP]** No framework-vs-backend separation (NoOpStore needed).

### Advertised vs reality

- [ ] **[GAP]** `bench_suite.rs:1-19` — Module doc claims about what each group "isolates" are false given the above.
	> SuGestion: rewrite the doc once the bugs are fixed.

### NIT

- [ ] **[NIT]** `bench_suite.rs:34` — `LOAD_EVENT_COUNTS` stops at 500.
	> SuGestion: extend to 100k.
- [ ] **[NIT]** `bench_suite.rs:251-257` — Not-found path grouped under `load_scaling/0`.
	> SuGestion: own group `bench_load_missing`.

---

# Suggested triage order

1. **Verify [BUG] items first** — especially `wee-events-restate` client/server name mismatch, the `Renderer` no-exhaustiveness story, and the `Revision` / `AggregateId` / `Entity::initialized()` unenforced invariants. Each has a runnable sin in `crates/sins/` proving the gap.
2. **Cut error sprawl.** Pick a real domain-vs-infrastructure boundary (probably `EventStore::Error = wee_events::Error` with `Custom(Box<dyn Error>)`) and delete every `into_store_error` helper. The `From` cycles fall out on their own.
3. **Delete what isn't earning its keep.** `service_builder` HList, codec HList, marker subtraits in sqlite strategies, `create.rs` builder chain, `EffectRouter` in restate, `Handles<C>` sandwich.
4. **Replace `join_all` with `JoinSet`** in the bench suite. Without this, every "concurrent" bench is fiction.
5. **Move turso adapter out of `wee-events-sqlite`** into its own crate.
6. **Add trybuild + UI tests for the macros.**
7. **Fix the `justfile`** so `fmt-check` actually checks.
8. **Cosmetic / NITs** last.

---

# Done

Completed in this branch. Details (was → is, bench evidence) in `tradeoffs-made.md`.

## Memory store core (early branch)

- [x] **[BUG]** `src/memory_store.rs:122-131` — `async fn load` held `std::sync::MutexGuard` across the body. Factored into a sync `load_sync` helper so the guard never crosses an `.await`.
- [x] **[BUG]** `src/memory_store.rs:139, 180-182` — Lock-in-lock between streams + generator. Replaced with `mint_event_ids` that acquires the generator lock once before the streams lock is taken.
- [x] **[BUG / DOC]** Reentrancy invariant for `EventStore` — added a `# Reentrancy` doc section to the trait stating handlers must not call back into the store mid-call.
- [x] **[PERF]** `src/memory_store.rs:127` (the central perf wart) — events wrapped in `Arc<RecordedEvent>`; load is now N refcount bumps instead of N deep copies. **bench: `load_scaling/500` -98%.**
- [x] **[PERF]** `src/memory_store.rs:194` — `ChangeSet.events` is now `Vec<Arc<RecordedEvent>>` sharing storage with the store-side stream; one allocation per event total (was three).
- [x] **[PERF / NIT]** `src/event.rs:14-18` — `EventData.encoding: Cow<'static, str>`. JSON/CBOR encoder constants flow through as `Cow::Borrowed`; SQL row reads use `Cow::Owned`. Zero alloc per event for the common path.
- [x] **[PERF]** `src/id.rs` `newtype_id!` — backs identifiers with `Cow<'static, str>`; `pub const fn new_const(&'static str)` is the zero-alloc constructor. Derive macros (`DomainEvent`, `Command`) emit `new_const(literal)`; every typed-event dispatch is now alloc-free. **bench: `creation/concentrated/32` -62%, `publish_batch/10` -11%, `publish_with_revision` -62%.**
- [x] **[ARCH]** `src/memory_store.rs:46-67` — `MemoryStoreBacking` is private. `from_shared` deleted. Sharing state between handles is `store.clone()` (one atomic; `MemoryStore` already `#[derive(Clone)]`). Conformance test updated.
- [x] **[PERF / partial]** Generator mutex acquired once per publish (instead of 2N times) — see `mint_event_ids`.
- [x] **[PERF]** `src/id.rs` — `Revision` and `AggregateId.aggregate_key` migrated from `Cow<'static, str>` to `Arc<str>`. Clone is now a refcount bump rather than a `String` allocation per clone. Workspace `serde` features include `"rc"` for `Arc<str>` serde support. **Bench: fat_payload `load_decode` -13% to -18% at 1MB/5MB; memory_store `creation/concentrated/32` -57% (cumulative).**
- [x] **[PERF]** `src/id.rs` — `AggregateType` lifted out of the `newtype_id!` macro into a bespoke `Arc<str>`-backed type. `AggregateId::clone()` (publish/load hot path) now two cheap Arc bumps with zero allocation. No `new_const` path needed in practice — grep confirmed no callers.
- [x] **[ARCH / docs]** `src/store.rs:54-75` — `impl<T: EventStore> EventStore for Arc<T>` blanket impl carries a footgun doc-comment: prefer `store.clone()` for `Clone` stores (`MemoryStore`); `Arc<Store>` is for stores whose internals aren't cheaply cloneable (`SqliteEventStore` holds async mutexes + connection pools). Not deleted — `examples/restate-counter:19` relies on `Arc<SqliteEventStore>` and the sqlite store isn't itself `Clone`; restructuring it is a separate refactor.

## Trait surface & error model

- [x] **[ARCH]** `src/store.rs:47-54` — `From<serde_json::Error>` dropped from the `EventStore::Error` bound. New `crate::Error::Custom(Box<dyn Error + Send + Sync>)` variant with `#[error(transparent)]` is the escape hatch for backends that need to embed JSON / libsql / arbitrary infra errors. `Error::custom(e)` helper.
- [x] **[ARCH]** `src/error.rs` — `RetryExhausted` variant pulled off `Error`, now its own `pub struct RetryExhausted { attempts, diagnostics }` with `#[error(...)]`. Surfaced via `Error::Custom`; construct with `Error::retry_exhausted(n, diag)`, recover with `error.downcast_ref::<RetryExhausted>()`. Structural variants (`RevisionConflict`, `EncodingMismatch`, `UnhandledEventType`) no longer share an enum with retry-policy outcomes.
- [x] **[ARCH]** `EventStoreErrorExt` **deleted**. `EventStore::Error` associated type **dropped** — both `load` and `publish` now return `Result<_, wee_events::Error>` directly. Backend-specific failures (libsql, ulid, serde_json, sqlite-internal `Error`) flow into `wee_events::Error::Custom` via per-backend `From<BackendError> for wee_events::Error` impls (sqlite has one; `MemoryStoreError` deleted entirely, MemoryStore returns `wee_events::Error` directly). Conformance suite matches on `wee_events::Error::RevisionConflict { .. }` without any downcast call. `From` bound graph collapsed: `Self::Error: From<crate::Error> + EventStoreErrorExt + Error + Send + Sync + 'static` → `Result<_, wee_events::Error>`.
- [x] **[ARCH]** `src/event.rs` / `src/renderer.rs` — Both `into_store_error<E>` helpers **deleted**. Replaced with a single blanket `impl<E: Error + Send + Sync + 'static> From<RenderError<E>> for crate::Error` that maps `UnhandledEventType` → structural variant and `ApplyFailed { source }` → `Error::custom(source)`. Each store's error type adds a thin blanket `From<RenderError<E>> for StoreError` that defers to `crate::Error`, so call sites just write `renderer.render(agg)?` — no more `map_err` closures.
- [x] **[ARCH]** `src/event.rs` — `DeserializeJsonError` **deleted**. `EventData::deserialize_json` now returns `Result<T, DecodeError>` and is a thin wrapper over `Encoding::Json.decode(self)`. The synthetic `serde_json::Error::io` smuggling is gone; the `DecodeError` enum carries the typed `EncodingMismatch { expected: Encoding, actual: Encoding }` variant introduced earlier, so there's nothing to translate. `Renderer<S, E = DecodeError>` default updated. All call sites (counter test, documents test, projections) compile unchanged.
- [x] **[ARCH]** `src/codec.rs:38-58` — `EncodeError`/`DecodeError` variants are now `#[cfg(feature = "cbor")]`-gated; encoder set admitted-closed. Adding protobuf/msgpack = new `codec/<name>.rs` module + new `Encoding` variant + new feature. See `_jer/codec-redesign.md`.
- [x] **[NIT]** `src/codec.rs:46-58` — `DecodeError::InvalidData` deleted.
- [x] **[ARCH]** `src/codec.rs:144-228` — HList (`Nil`/`Cons`/`DecoderList`/`EventDecoders`) deleted; runtime dispatch is `Encoding::decode(...)` — closed match on a feature-gated enum.
- [x] **[ARCH]** `src/codec.rs:21-36` — `EncodesEvents` collapsed from `type Encoder; fn event_encoder()` to `fn encoding(&self) -> Encoding`. Drove `wee-events-sqlite::EventStore<S, C, W>` → `EventStore<S, C>` (~290 lines of typestate builder removed).

## service_builder / service

- [x] **[ARCH/WONTFIX]** `src/service_builder.rs:181-205` — `EmptyHandlers` / `HandlerList` / `Here` / `There<Idx>` HList kept deliberately. The reviewer-suggested `HashMap<TypeId, Box<dyn ...>>` replacement would be ~⅒ the code, but it trades the most useful property the service surface has: **"did I wire this command?"** is a compile-time check today (via the `Handles<C>` bound + `Here`/`There` selector) and would become a runtime panic / `Err(NoHandler)` with the map. For an event-sourcing framework where service-author wiring mistakes are a top bug class, the type-level proof is worth the boilerplate. Runtime cost of the HList vs the map is irrelevant (both negligible next to the publish RTT); the difference is purely when the mistake surfaces.
- [x] **[ARCH]** `src/service_builder.rs:1, :51` — Fixed the dishonest "no type erasure" claim in the module header and the `BoxFuture` block comment. The dispatch *is* static (which `Fn`, which `HandleCommand` impl resolves at compile time) — but `BoxFuture<'a, T>` is `Pin<Box<dyn Future + Send + 'a>>`, which *is* type erasure of the future plus a heap allocation per handler call. The comment now states why the box exists (lifetime-polymorphic `for<'a> Fn(&'a Ctx, ...) -> impl Future + 'a` is inexpressible) instead of pretending the boxes aren't erasure. Behaviour unchanged; this is a comments-only fix. Dropping the boxes for an explicit state-machine future is deferred — would overlap with the `spec.rs` redundant-aliases cleanup.
- [x] **[ARCH]** `src/service_builder.rs:1` — `#![allow(private_bounds, private_interfaces)]` **deleted**. The allow was dead code: every macro-internal type in the module is already `pub` + `#[doc(hidden)]`, so neither lint had anything to fire on. Removed; `cargo check --workspace --all-features` clean.
- [x] **[PERF]** `src/service_builder.rs:305-344` — `BuiltService::load` / `execute` now take `AggregateId` by value; the hidden `let id = id.clone();` is deleted. Migrated the full stack: `TypedService::load/execute` trait, `__private::DispatchCommand::dispatch_command` trait, the two `service!` macro emit sites (`service_macro.rs:546, 654`), and the `RestateClient` impls in `wee-events-restate/src/client.rs`. ~24 caller sites rewritten from `service.load(&id)` to `service.load(id.clone())` — clone now visible at the call site. Post-`Arc<str>` the clone is two atomic increments; the value here is API honesty, not raw cycles. UI snapshot re-blessed. 31 test binaries pass.
- [x] **[FOOTGUN]** `src/publisher.rs:22-52` — `Publisher::publish` guard kept mandatory. The `HandlerEnv: EventStore` backdoor (handler_env.rs:39-61) is **deleted** — handlers can no longer call `env.publish(...)` directly to bypass the revision guard. Workspace + all tests still compile (nothing depended on the impl). Note: the broader underlying issue (calling `MemoryStore::publish` directly with `PublishOptions::default()`) is unchanged — that's a framework-boundary concern, not a service-author footgun.

## Renderer

- [x] **[ARCH/FOOTGUN]** `src/renderer.rs` — Four collections (`reducers` HashMap + `pattern_reducers` Vec + `ignored` HashSet + `ignored_patterns` Vec) and the implicit precedence between them (exact-reducer > exact-ignore > pattern-reducer > pattern-ignore, never documented) **collapsed** to one ordered `Vec<(EventPattern, Action)>` where `Action ∈ {Reduce(fn), Ignore}`. Render walks the vec; **first matching rule wins**, in registration order — same semantics as `match` arms. Users now control precedence by writing rules most-specific-first; no hidden ranking. Dropped the 8-entry-point API (4 ops × builder/mutating) down to 4 (`with`/`register` + `ignore`/`register_ignore`). 4 regression tests in `renderer::precedence_tests` pin down the new contract: earlier-specific beats later-glob, earlier-ignore beats later-reducer, earlier-reducer beats later-ignore, no-match still fails. No behaviour change for any in-tree caller — none registered overlapping reducer + ignore rules.

## Identity & data

- [x] **[BUG]** `src/id.rs:65-85` — Lex-comparable invariant **enforced**. `Revision::new` deleted; `From<&str>` / `From<String>` deleted. Construction is now: `Revision::from_ulid(Ulid)` (infallible, type-carries-invariant), `Revision::generate()` (fresh ULID for tests/ad-hoc), `Revision::zero()` (sentinel). Untrusted strings go through `TryFrom<&str>` / `TryFrom<String>` / `FromStr` which validate as 26-char Crockford ULID (or the all-zero sentinel) and yield `RevisionParseError`. Custom `Deserialize` validates too — `serde_json::from_str::<Revision>("\"zzz\"")` is now an error. Internal call sites converted: `MemoryStore::mint_event_ids` passes the freshly-generated `Ulid` straight in; SQLite `EventStore::generate_ulid` returns `Ulid`; DB-row readers go through `try_from` and report parse failures as `Error::Internal` (data-corruption case). 4 regression tests in `id::revision_tests` cover garbage strings, valid ULIDs, and the deserialize path.

## Misc core

- [x] **[NIT]** `src/memory_store.rs` — `mint_event_ids` now writes each ULID into a stack `[u8; ULID_LEN]` via `Ulid::array_to_str`, then constructs `EventId` / `Revision` from `&str`. Saves one `String` alloc per minted ID (Revision was 2 allocs, now 1; EventId stays at 1 — already fit into `Cow::Owned`).
- [x] **[FOOTGUN]** `src/memory_store.rs` — Fixed. `publish_sync` now uses `streams.get(id)` first; only calls `stream_for` (which inserts) when either the aggregate already exists or the caller's `expected_revision` is zero/absent. A failed conflict-check on a brand-new aggregate returns `Err` without leaking an entry. TOCTOU window between `get` and inner-write-lock is closed by re-checking the revision inside the lock. Verified via inline repro of `crates/sins/src/bin/memory_store_phantom_aggregate.rs`.
- [x] **[PERF]** `src/memory_store.rs:56` — Now `DashMap<AggregateId, Arc<parking_lot::RwLock<Vec<Arc<RecordedEvent>>>>>`. DashMap gives per-shard locks; inner `RwLock` is acquired *after* the shard guard is dropped, so shard-mate aggregates never wait on each other and concurrent loads of one aggregate share the inner read lock. ULID generator switched to `parking_lot::Mutex`. **Bench (vs `baseline`): publish_batch/{1,10,50} -43/-45/-48%; partition_write/contention/{8,16,32} -39/-51/-56%; partition_read/concentrated/32 -85%; mixed/32r_32w -83%; load_scaling/500 -98%.**
- [x] **[PERF]** `src/renderer.rs:226-228` — Success path no longer clones. `Renderer::render` now consumes the `Aggregate` (via new `Aggregate::into_parts`); `aggregate_id` and `revision` move into `Entity` instead of being cloned. `RenderEventContext::new` (error path) left as-is per FEEDBACK note. API change: `render(&Aggregate)` → `render(Aggregate)`. Internal callers (`wee-events-sqlite::projections`) + tests + sins updated.
- [x] **[PERF / PARTIAL]** `src/aggregate.rs:42-45`, `src/memory_store.rs` revision clones — memory_store conflict-check happy path now compares via `Option<&Revision>` and only clones on the error path. `aggregate.rs::from_shared_events` clones are cheap Arc bumps post-Arc<str>. Verified no bench regression vs baseline.

## SQLite — mega-struct + builder

- [x] **[ARCH]** `src/event_store/store.rs` — Type-state builder + 6 phantom-state structs + 5 backend payload structs + `BackendBinding` trait + 3 trivial backend impl files **deleted (~360 lines)**. Replaced with 5 concrete inherent constructors on `EventStore<S, C>`: `open_local(path, strategy)`, `open_in_memory(strategy)`, `open_sqld_default(provisioner, strategy)`, `open_sqld_namespaced(provisioner, strategy)`, `open_turso(provisioner, strategy)`. Each enforces appropriate strategy bounds on its impl block. Encoding moved off the constructor signature into a chainable `with_encoding(self, Encoding) -> Self` setter — defaults to Json. `from_catalog` remains as the escape hatch. All ~21 call sites across tests/benches migrated; 219 tests pass. `W` generic was already gone from earlier work.
- [x] **[FOOTGUN/PARTIAL]** `src/event_store/mod.rs:9-13` — `SqliteEventStore` default-C generic now tracks S (`LocalPartitionCatalog<S>` instead of hardcoded `LocalPartitionCatalog<GlobalStrategy>`). Fixes a subtle bug where `SqliteEventStore<TypeStrategy>` produced a store whose strategy and catalog disagreed. `W` generic is already gone from earlier work; remaining aliases (`LocalStore<S>`, `InMemoryStore<S>`, `SingleRemoteStore<S,R>`, `NamedRemoteStore<S,R>`, `RemoteStore<S,C>`) are typed return shapes for builder paths, not redundant names. Keeping them.
- [x] **[ARCH/PARTIAL]** `src/event_store/store.rs:61-63` — `known_partitions` (`AsyncMutex<BTreeSet>`) deleted; `all_known_partitions` now unions `catalog.partitions()` with `connections.lock().await.keys()`. Two `remember_partition` call sites removed. `connections` stays `AsyncMutex` — must remain async because my open-race fix holds it across `catalog.ensure_target_for_partition().await`, `open_target().await`, `prepare_connection_for_partition().await`. The earlier feedback note ("doesn't `.await` while held") was true before the race fix; reverting to a sync `Mutex` would reintroduce the SQLITE_BUSY panic.

## SQLite — concurrency / correctness

- [x] **[BUG]** `src/event_store/store.rs:187-232` — `ensure_partition_open` / `open_partition_if_exists` race: lock now held across the whole open + prepare + insert phase, so concurrent first-time openers serialise; second caller observes the cached connection. Closes the local-sqlite migration-DDL collision (was the root cause of bench `bwj88ka8g` SQLITE_BUSY panic) and the Turso `create_database` double-fire concern.
- [x] **[BUG]** `src/event_store/store.rs:633-653` — Lazy-partition-not-ready detector no longer string-matches over the *entire* `libsql::Error` Display; now narrowed to remote-transport variants only (`Hrana`, `WriteDelegation`, `ConnectionFailed`). Phrase tolerance widened to also accept `does not exist`, `not found`, and `404`. Local sqlite errors can no longer accidentally match. Still string-based — sqld returns 404+JSON, not a code — comment documents the upstream fragility.
- [x] **[BUG]** `src/event_store/store.rs` — Retry jitter migrated from `SystemTime::subsec_nanos()` (phase-locks under contention) to a process-global atomic xorshift64* PRNG (`next_jitter`). Seeded from system time on first call, advances lock-free with Relaxed ordering. Both `retry_delay` (conflict-retry) and `busy_retry_delay` (BUSY/LOCKED) use it. No new deps.
- [x] **[NIT]** `src/event_store/store.rs:1006-1073` — three `format!`-built INSERT statements replaced by three `const &'static str` (`SQL_INITIAL`, `SQL_EXACT`, `SQL_ADVANCE`). No per-call concatenation.
- [x] **[BUG]** `event_store/store.rs` connection cache race — `ensure_partition_open` / `open_partition_if_exists` dropped the `connections` lock between the negative lookup and the insert, so concurrent first-time opens both ran migration DDL on independent connections → `SqliteFailure(5, "database is locked")` panics during bench warmup. Fixed: hold `connections.lock()` across the whole open + prepare + insert phase; second caller now sees the cached connection. Defence in depth: BUSY/LOCKED retry loop around `try_publish_once` (8 attempts, 5→250ms backoff + jitter, independent of conflict-retry counter); `busy_timeout` raised 5s → 30s. Verified: `local_by_type/creation/concentrated/{2..32}` runs to completion (bench `b2uc3dn1t`).

## SQLite — partitioning + persistence

- [x] **[BUG]** `src/event_store/strategies/hashed.rs:84-99` — FNV-1a now length-prefixes the aggregate type (4 LE bytes of `agg_type.len()` then type bytes then key bytes, no separator). `("foo:", "bar")` and `("foo", ":bar")` hash distinctly. Regression tests added (`type_key_boundary_does_not_collide`, `distinct_aggregates_have_distinct_hashes`).
- [x] **[NIT]** `src/database.rs:110-119` — Guard widened: now skips `PRAGMA journal_mode=WAL` when the current mode is already `wal` (previously only skipped on `memory`). One fewer round-trip per re-opened connection.

## Restate counter example (directory deleted in commit 83c7ef3)

- [x] **[BUG]** `counter-events.db` — Committed SQLite database, not gitignored. Moot post-deletion.
- [x] **[FOOTGUN]** `src/main.rs:32-39` — `listen_and_serve("0.0.0.0:9080".parse().unwrap())` — no graceful shutdown, no error handling on the future. Moot post-deletion.
- [x] **[NIT]** `src/main.rs:17` — `tracing_subscriber::fmt::init()` for nothing. Moot post-deletion.
- [x] **[FOOTGUN]** `src/main.rs:23-26` — `.expect(...)` on operational errors. Moot post-deletion.
- [x] **[ARCH]** `src/services/counter.rs:1-72` — `Result<(), ServiceError<<R::Store as EventStore>::Error>>` verbatim 3×. Moot post-deletion.
- [x] **[FOOTGUN]** `src/services/counter.rs:35-49` — `reset` returns `Ok(())` without publishing on zero. Subtle concurrent-ordering implication. Moot post-deletion.
- [x] **[ARCH]** `src/services/audit_log.rs:1-27` — `WorkflowContext` unused; just `println!`s. Moot post-deletion.
- [x] **[NIT]** `src/state.rs:21-26` — `EventDecoders::new().with(JsonDecoder).with(CborDecoder)` rebuilt per call. Moot post-deletion.
- [x] **[NIT]** `src/randomiser/system.rs:7-16` — "Random" = `now_nanos() % span`. Biased. Moot post-deletion.
- [x] **[FOOTGUN]** `src/commands/*.rs` — Three command structs, each with a hand-rolled `impl Command`; the derive macro exists but isn't used. Moot post-deletion.

## Workspace / build / docs

- [x] **[FOOTGUN]** `Cargo.toml` — `[workspace.lints.clippy]` added: `pedantic = warn` baseline with narrow allows for `missing_errors_doc`, `missing_panics_doc`, `too_many_lines`. Each crate references via `[lints] workspace = true`. All 84 pedantic warnings dealt with — substantive ones (casts, wildcard arms, identical match bodies, redundant rebinds, unused async, by-value-but-borrowed params, missing `#[must_use]`) hand-fixed; only doc-section + line-count noise is allowed. `cargo clippy --workspace --all-features` is now warning-clean.

## Benchmarks

- [x] **[BUG]** `bench_suite.rs:115, 304, 347, 386, 516` — `join_all` polls sequentially on one task; "concurrent n=32" measures 32 sequential calls. Replaced with `JoinSet::spawn` + `join_next` — actual parallelism on the multi-thread runtime.
- [x] **[BUG]** `bench_suite.rs:32` — `CONCURRENCY_LEVELS = &[2, 4, 8, 16, 32]` is a meaningful axis now that `JoinSet` lands real concurrency.
- [x] **[BUG]** `bench_suite.rs:373-394` — `partition_write/contention` no longer false advertising: spawns N writers, gates on a `Barrier`, starts simultaneously.
- [x] **[BUG]** `bench_suite.rs:233-240` — `steady_state/append` migrated to `iter_batched(fresh_aggregate, work, BatchSize::SmallInput)`; no monotonic state growth across iterations.
- [x] **[BUG]** `bench_suite.rs:209-218` — `publish_with_revision` same `iter_batched` pattern.
- [x] **[BUG]** `bench_suite.rs:179-194` — `publish_batch` same fix.
- [x] **[BUG]** `bench_suite.rs:294-316, 337-359` — pre-seeded aggregate benches no longer accumulate ~10k publishes across the run.
- [x] **[BUG]** `bench_suite.rs:387` — `let _ = store.publish(...).await;` no longer discards the `Result`; `.expect(...)` panics on failure.
- [x] **[NOISE]** `bench_suite.rs` — All simple `iter(|| async {})` benches (`bench_create_aggregate`, `bench_create_spread`, `bench_create_concentrated`, `bench_load_scaling/0`) migrated to `iter_batched(setup, routine, BatchSize::SmallInput)`. Setup builds IDs + clones the pre-encoded raw event template; routine is the timed publish/spawn. `iter_custom` bodies now also clone raw event vecs *before* `Instant::now()`.
- [x] **[NIT]** `bench_suite.rs:105` — Comment claiming `iter_batched` is used is now true.
- [x] **[NOISE]** `bench_suite.rs` — `make_spread_id` / `make_concentrated_id` / `make_test_id` now use a process-global `AtomicU64` counter + `format!("{:016x}", ...)` instead of `ulid::Ulid::new()` (microseconds + ULID encoding alloc). One relaxed atomic increment + a single 16-char string alloc per call.
- [x] **[NOISE]** `bench_suite.rs` — JSON-encoded raw event templates are built **once per bench function** via `prebuilt_raw_events(n)`, captured by reference in the iter closure, and `.clone()`d either in `iter_batched` setup or pre-`Instant::now()`. The serde JSON encode happens outside every measurement.
- [x] **[NOISE]** `bench_suite.rs` — Outer per-iter `Arc::clone(store)` removed throughout. Closure captures `store: &Arc<S>`; only spawned tasks clone (required for the `'static` future bound).
- [x] **[NOISE]** `bench_suite.rs` — `bench_read_spread` / `bench_read_concentrated` now pre-build IDs into `Arc<[AggregateId]>` once outside the iter; the iter closure captures `&ids` and only the per-spawn `id.clone()` (Arc<str> refcount bump) happens inside the timed region.
- [x] **[METHOD]** `bench_suite.rs:599` — Criterion `sample_size`, `measurement_time`, `warm_up_time`, `noise_threshold` configured per group instead of defaults.
