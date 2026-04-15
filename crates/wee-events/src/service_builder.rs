#![allow(private_bounds, private_interfaces)]

//! Portable async-factory runtime for building typed services.
//!
//! Provides a `ServiceBuilder<S>` that accumulates a loader function and typed
//! handler functions. The `.build(factory)` method produces a `BuiltService`
//! that has `load` and `execute` methods and implements `Handles<C>` for each
//! registered command — all verified at compile time via a type-level handler
//! list.
//!
//! ## Handler signature
//!
//! ```ignore
//! async fn my_handler(ctx: &Ctx, entity: &Entity<S>, cmd: C) -> crate::Result<Entity<S>>
//! ```
//!
//! ## Example
//!
//! ```ignore
//! let service = ServiceBuilder::<Counter>::new()
//!     .with_loader(load_counter)
//!     .with_handler::<Increment, _>(increment)
//!     .with_handler::<Adjust, _>(adjust)
//!     .build(|| async { Ok(TestContext::default()) });
//!
//! let entity = service.load(&id).await?;
//! let entity = service.execute(&id, Increment { amount: 2 }).await?;
//! ```
//!
//! ## TypedService compatibility
//!
//! `BuiltService` provides `load` and `execute` as inherent async methods with
//! the same signatures as `TypedService<S>`. A blanket `TypedService` impl
//! cannot be provided directly because Rust's trait-impl rules prohibit adding
//! the `HandleCommand<C, Idx, Ctx, S>` bound in the trait method body.
//!
//! Macro-generated service structs (Task 3) can delegate to `BuiltService` and
//! implement `TypedService` at that point.

use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;

use crate::entity::Entity;
use crate::id::AggregateId;
use crate::service::Handles;
use crate::Command;

// ---------------------------------------------------------------------------
// Erased future type alias
// ---------------------------------------------------------------------------

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

// ---------------------------------------------------------------------------
// Fully-erased types for macro-generated services
//
// Macro-generated services need a struct with no `Ctx` type parameter so that
// `build` can be a normal method that constrains `Ctx` via inference from the
// factory/loader/handler arguments.
//
// Both the context AND each command type are fully erased using `dyn Any`.
// The concrete context is produced by the factory as a `Box<dyn Any + Send>`.
// The loader and handlers downcast the context back to their concrete `Ctx`.
//
// This design adds a small allocation + dynamic dispatch overhead in exchange
// for a clean, simple struct type with no unnameable generic parameters.
// ---------------------------------------------------------------------------

/// A factory that produces a reference-counted, fully type-erased context.
///
/// Using `Arc` allows the context to be cheaply shared between the loader
/// and handler within a single `execute` call without requiring `Ctx: Clone`.
pub type ErasedFactory = Box<
    dyn Fn() -> BoxFuture<'static, crate::Result<std::sync::Arc<dyn std::any::Any + Send + Sync>>>
        + Send
        + Sync,
>;

/// A type-erased loader that takes an `Arc`-wrapped context and an owned `AggregateId`.
///
/// Using `Arc` for the context and owned `AggregateId` lets the returned
/// `BoxFuture<'static, ...>` own all its data, avoiding unsatisfiable lifetime
/// constraints on the erased closure.
pub type ErasedLoader<S> = Box<
    dyn Fn(
            std::sync::Arc<dyn std::any::Any + Send + Sync>,
            AggregateId,
        ) -> BoxFuture<'static, crate::Result<Entity<S>>>
        + Send
        + Sync,
>;

/// A type-erased command handler that takes an `Arc`-wrapped context, owned entity,
/// and boxed command.
pub type ErasedHandler<S> = Box<
    dyn Fn(
            std::sync::Arc<dyn std::any::Any + Send + Sync>,
            Entity<S>,
            Box<dyn std::any::Any + Send + 'static>,
        ) -> BoxFuture<'static, crate::Result<Entity<S>>>
        + Send
        + Sync,
>;

/// Wraps a typed async factory into an `ErasedFactory`.
///
/// The produced context is wrapped in `Arc<dyn Any + Send + Sync>` so it can
/// be cheaply shared between the loader and handler within a single `execute`.
pub fn erase_factory<Ctx, F, Fut>(factory: F) -> ErasedFactory
where
    Ctx: std::any::Any + Send + Sync + 'static,
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = crate::Result<Ctx>> + Send + 'static,
{
    use std::sync::Arc;
    Box::new(move || {
        let fut = factory();
        Box::pin(async move {
            let ctx: Ctx = fut.await?;
            let arc: Arc<dyn std::any::Any + Send + Sync> = Arc::new(ctx);
            Ok(arc)
        })
    })
}

/// Wraps a typed async loader function into an `ErasedLoader`.
///
/// Takes an `Arc`-wrapped erased context and an owned `AggregateId`. Inside
/// an `async move` block (which owns all data), uses `LoaderBridge` to invoke
/// the loader and await the result. The returned future is `'static` because
/// it owns all the data it references.
///
/// Panics if the context type does not match `Ctx` — that would be a bug.
pub fn erase_loader<Ctx, S, F>(loader: F) -> ErasedLoader<S>
where
    Ctx: std::any::Any + Send + Sync + 'static,
    S: Send + Sync + 'static,
    F: Send + Sync + 'static,
    for<'a> &'a F: LoaderBridge<'a, Ctx, S>,
{
    use std::sync::Arc;
    let loader = Arc::new(loader);
    Box::new(
        move |ctx_arc: Arc<dyn std::any::Any + Send + Sync>, id: AggregateId| {
            let ctx: Arc<Ctx> = ctx_arc
                .downcast::<Ctx>()
                .expect("erase_loader: context type mismatch (this is a bug)");
            let loader = Arc::clone(&loader);
            Box::pin(async move {
                <&F as LoaderBridge<'_, Ctx, S>>::call(&*loader, &*ctx, &id).await
            })
        },
    )
}

/// Wraps a typed async handler function into an `ErasedHandler`.
///
/// Uses `HandlerBridge` inside an `async move` block for the same reasons as
/// `erase_loader`. All data is owned by the async block, making the future
/// `'static`. Panics on type mismatch.
pub fn erase_handler<Ctx, S, C, F>(handler: F) -> ErasedHandler<S>
where
    Ctx: std::any::Any + Send + Sync + 'static,
    S: Send + Sync + 'static,
    C: std::any::Any + Send + 'static,
    F: Send + Sync + 'static,
    for<'a> &'a F: HandlerBridge<'a, Ctx, S, C>,
{
    use std::sync::Arc;
    let handler = Arc::new(handler);
    Box::new(
        move |ctx_arc: Arc<dyn std::any::Any + Send + Sync>,
              entity: Entity<S>,
              cmd_any: Box<dyn std::any::Any + Send + 'static>| {
            let ctx: Arc<Ctx> = ctx_arc
                .downcast::<Ctx>()
                .expect("erase_handler: context type mismatch (this is a bug)");
            let cmd = *cmd_any
                .downcast::<C>()
                .expect("erase_handler: command type mismatch (this is a bug)");
            let handler = Arc::clone(&handler);
            Box::pin(async move {
                <&F as HandlerBridge<'_, Ctx, S, C>>::call(&*handler, &*ctx, &entity, cmd).await
            })
        },
    )
}

// ---------------------------------------------------------------------------
// HandlerBridge — lifetime-polymorphic glue.
//
// Rust cannot express `F: for<'a> Fn(&'a Ctx, ...) -> Fut<'a>` where `Fut`
// varies with `'a`. The workaround: implement the bridge on `&'a F` so the
// returned future can borrow `ctx` and `entity` for exactly `'a`.
// ---------------------------------------------------------------------------

#[doc(hidden)]
pub trait HandlerBridge<'a, Ctx: 'a, S: 'a, C>: Sized {
    fn call(
        f: Self,
        ctx: &'a Ctx,
        entity: &'a Entity<S>,
        cmd: C,
    ) -> BoxFuture<'a, crate::Result<Entity<S>>>;
}

impl<'a, Ctx, S, C, F, Fut> HandlerBridge<'a, Ctx, S, C> for &'a F
where
    Ctx: 'a,
    S: 'a,
    F: Fn(&'a Ctx, &'a Entity<S>, C) -> Fut,
    Fut: Future<Output = crate::Result<Entity<S>>> + Send + 'a,
{
    fn call(
        f: Self,
        ctx: &'a Ctx,
        entity: &'a Entity<S>,
        cmd: C,
    ) -> BoxFuture<'a, crate::Result<Entity<S>>> {
        Box::pin(f(ctx, entity, cmd))
    }
}

// ---------------------------------------------------------------------------
// LoaderBridge — same lifetime trick for the loader function.
// ---------------------------------------------------------------------------

#[doc(hidden)]
pub trait LoaderBridge<'a, Ctx: 'a, S: 'a>: Sized {
    fn call(
        f: Self,
        ctx: &'a Ctx,
        id: &'a AggregateId,
    ) -> BoxFuture<'a, crate::Result<Entity<S>>>;
}

impl<'a, Ctx, S, F, Fut> LoaderBridge<'a, Ctx, S> for &'a F
where
    Ctx: 'a,
    S: 'a,
    F: Fn(&'a Ctx, &'a AggregateId) -> Fut,
    Fut: Future<Output = crate::Result<Entity<S>>> + Send + 'a,
{
    fn call(
        f: Self,
        ctx: &'a Ctx,
        id: &'a AggregateId,
    ) -> BoxFuture<'a, crate::Result<Entity<S>>> {
        Box::pin(f(ctx, id))
    }
}

// ---------------------------------------------------------------------------
// Type-level handler list
// ---------------------------------------------------------------------------

/// Terminator for the type-level handler list.
#[doc(hidden)]
pub struct EmptyHandlers;

/// A cons cell in the type-level handler list.
///
/// `C` is the command type handled at this node, `H` is the handler function,
/// and `Tail` is the rest of the list.
#[doc(hidden)]
pub struct HandlerList<C, H, Tail> {
    handler: H,
    tail: Tail,
    _cmd: PhantomData<fn(C)>,
}

// ---------------------------------------------------------------------------
// Selector indices — the standard HList trick for avoiding overlapping impls.
//
// Without these, `impl HandleCommand for HandlerList<C, ...>` and
// `impl HandleCommand for HandlerList<Other, ...>` overlap because Rust cannot
// prove `C ≠ Other` at coherence-check time. Adding a phantom `Idx` parameter
// to the trait makes the two impls target distinct types (`Here` vs
// `There<Idx>`), which are always disjoint.
// ---------------------------------------------------------------------------

/// The matching handler is at the head of this list.
#[doc(hidden)]
pub struct Here;

/// The matching handler is deeper in the tail at position `Idx`.
#[doc(hidden)]
pub struct There<Idx>(PhantomData<Idx>);

// ---------------------------------------------------------------------------
// HandleCommand<C, Idx, Ctx, S>
// ---------------------------------------------------------------------------

/// Looks up and invokes the handler for `C` in a type-level handler list.
///
/// `Idx` is a phantom position selector inferred by the compiler; callers
/// never need to specify it.
#[doc(hidden)]
pub trait HandleCommand<C, Idx, Ctx, S> {
    fn handle<'a>(
        &'a self,
        ctx: &'a Ctx,
        entity: &'a Entity<S>,
        cmd: C,
    ) -> BoxFuture<'a, crate::Result<Entity<S>>>;
}

/// `C` is the head of this list — invoke directly.
impl<C, H, Tail, Ctx, S> HandleCommand<C, Here, Ctx, S> for HandlerList<C, H, Tail>
where
    Ctx: Send + Sync + 'static,
    S: Send + Sync + 'static,
    C: Send + 'static,
    H: Send + Sync + 'static,
    for<'a> &'a H: HandlerBridge<'a, Ctx, S, C>,
{
    fn handle<'a>(
        &'a self,
        ctx: &'a Ctx,
        entity: &'a Entity<S>,
        cmd: C,
    ) -> BoxFuture<'a, crate::Result<Entity<S>>> {
        <&'a H as HandlerBridge<'a, Ctx, S, C>>::call(&self.handler, ctx, entity, cmd)
    }
}

/// `C` is not the head — recurse into the tail.
impl<C, Other, H, Tail, Idx, Ctx, S> HandleCommand<C, There<Idx>, Ctx, S>
    for HandlerList<Other, H, Tail>
where
    Tail: HandleCommand<C, Idx, Ctx, S>,
{
    fn handle<'a>(
        &'a self,
        ctx: &'a Ctx,
        entity: &'a Entity<S>,
        cmd: C,
    ) -> BoxFuture<'a, crate::Result<Entity<S>>> {
        self.tail.handle(ctx, entity, cmd)
    }
}

// ---------------------------------------------------------------------------
// Handles<C> for BuiltService
//
// We implement the `Handles<C>` marker from `service.rs` for `BuiltService`
// when the handler list contains `C`. The `Idx` selector is needed to avoid
// overlapping impls but is a phantom type — it does not appear on `BuiltService`
// itself and is not visible to callers.
//
// Rust's E0207 rule ("unconstrained type parameter") prevents `Idx` from
// appearing only in a where-clause bound. The workaround: add `Idx` to a
// private sealed marker trait `ContainsHandler<C, Idx>` and implement
// `Handles<C>` from it. The `Idx` in `ContainsHandler`'s impls IS constrained
// by the impl's self-type chain.
// ---------------------------------------------------------------------------

/// Sealed marker: the handler list `Self` contains a handler for `C` at
/// position `Idx` in the list.
trait ContainsHandler<C, Idx> {}

impl<C, H, Tail> ContainsHandler<C, Here> for HandlerList<C, H, Tail> {}

impl<C, Other, H, Tail, Idx> ContainsHandler<C, There<Idx>> for HandlerList<Other, H, Tail> where
    Tail: ContainsHandler<C, Idx>
{
}

// Blanket impl: `BuiltService` handles `C` at position `Idx` when `Handlers`
// contains `C` at that position.
//
// `Idx` appears in the trait `Handles<C, Idx>` being implemented, which
// satisfies Rust's E0207 constraint (every type parameter must appear in
// the trait, self-type, or their associated types).
impl<C, Idx, Ctx, S, L, F, Handlers> Handles<C, Idx> for BuiltService<Ctx, S, L, F, Handlers> where
    Handlers: ContainsHandler<C, Idx>
{
}

// ---------------------------------------------------------------------------
// Factory type alias — erases the concrete factory future type.
// ---------------------------------------------------------------------------

/// A boxed async factory that produces a fresh `Ctx` per call.
///
/// Erasing the future type here keeps `BuiltService`'s type signature clean.
pub type Factory<Ctx> =
    Box<dyn Fn() -> BoxFuture<'static, crate::Result<Ctx>> + Send + Sync>;

// ---------------------------------------------------------------------------
// BuiltService
// ---------------------------------------------------------------------------

/// A compiled service produced by `ServiceBuilder::build`.
///
/// - `load` and `execute` are inherent async methods.
/// - `Handles<C>` is implemented for every command registered with
///   `with_handler` — the compiler rejects `execute` calls with unregistered
///   command types.
/// - `Send + Sync` when `L` and `Handlers` are `Send + Sync`.
pub struct BuiltService<Ctx, S, L, F, Handlers> {
    factory: F,
    loader: L,
    handlers: Handlers,
    _ctx: PhantomData<fn() -> Ctx>,
    _state: PhantomData<fn() -> S>,
}

// SAFETY: `Ctx` and `S` appear only in `PhantomData<fn() -> T>`, which is
// covariant-to-contravariant and therefore always `Send + Sync` regardless of
// whether `T` itself is `Send`. The only real data (`factory`, `loader`,
// `handlers`) are guarded by explicit `Send`/`Sync` bounds below.
unsafe impl<Ctx, S, L, F, Handlers> Send for BuiltService<Ctx, S, L, F, Handlers>
where
    F: Send,
    L: Send,
    Handlers: Send,
{
}

unsafe impl<Ctx, S, L, F, Handlers> Sync for BuiltService<Ctx, S, L, F, Handlers>
where
    F: Sync,
    L: Sync,
    Handlers: Sync,
{
}

impl<Ctx, S, L, Handlers> BuiltService<Ctx, S, L, Factory<Ctx>, Handlers>
where
    Ctx: Send + Sync + 'static,
    S: Send + Sync + 'static,
    L: Send + Sync + 'static,
    Handlers: Send + Sync + 'static,
    for<'a> &'a L: LoaderBridge<'a, Ctx, S>,
{
    /// Load the current entity state for the given aggregate.
    ///
    /// Calls the factory to get a fresh context, then the loader.
    pub fn load(
        &self,
        id: &AggregateId,
    ) -> impl Future<Output = crate::Result<Entity<S>>> + Send + '_ {
        let id = id.clone();
        async move {
            let ctx = (self.factory)().await?;
            <&L as LoaderBridge<'_, Ctx, S>>::call(&self.loader, &ctx, &id).await
        }
    }

    /// Execute a typed command against the aggregate.
    ///
    /// Requires `Self: Handles<C>` — satisfied at compile time when `C` was
    /// registered with `ServiceBuilder::with_handler`.
    ///
    /// Calls the factory (fresh context), loader (current entity), then the
    /// registered handler for `C`.
    // `Handles<C, Idx>` provides the user-facing compile error for unregistered
    // commands; `HandleCommand` provides the actual dispatch mechanism. Both are
    // needed because the trait-level `Handles` bound cannot imply the internal
    // `HandleCommand` bound without a blanket impl that Rust's coherence rules
    // reject.
    pub fn execute<C, Idx>(
        &self,
        id: &AggregateId,
        cmd: C,
    ) -> impl Future<Output = crate::Result<Entity<S>>> + Send + '_
    where
        C: Command + serde::Serialize + Send + 'static,
        Self: Handles<C, Idx>,
        Handlers: HandleCommand<C, Idx, Ctx, S>,
    {
        let id = id.clone();
        async move {
            let ctx = (self.factory)().await?;
            let entity =
                <&L as LoaderBridge<'_, Ctx, S>>::call(&self.loader, &ctx, &id).await?;
            self.handlers.handle(&ctx, &entity, cmd).await
        }
    }
}

// ---------------------------------------------------------------------------
// ServiceBuilder
// ---------------------------------------------------------------------------

/// Builds a `BuiltService` by accumulating a loader and typed handler functions.
///
/// The type-level handler list is built incrementally via `with_handler`. Each
/// call prepends a new `HandlerList<C, H, Tail>` node to the `Handlers` type
/// parameter, so the final `BuiltService` carries the complete list in its type.
pub struct ServiceBuilder<S, L = (), Handlers = EmptyHandlers> {
    loader: L,
    handlers: Handlers,
    _state: PhantomData<fn() -> S>,
}

impl<S> ServiceBuilder<S, (), EmptyHandlers> {
    /// Create a new builder with no loader and no handlers registered.
    pub fn new() -> Self {
        Self {
            loader: (),
            handlers: EmptyHandlers,
            _state: PhantomData,
        }
    }
}

impl<S> Default for ServiceBuilder<S, (), EmptyHandlers> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S, L, Handlers> ServiceBuilder<S, L, Handlers> {
    /// Register the loader function.
    ///
    /// Signature: `async fn(ctx: &Ctx, id: &AggregateId) -> crate::Result<Entity<S>>`
    pub fn with_loader<NewL>(self, loader: NewL) -> ServiceBuilder<S, NewL, Handlers> {
        ServiceBuilder {
            loader,
            handlers: self.handlers,
            _state: PhantomData,
        }
    }

    /// Register a typed async handler for command `C`.
    ///
    /// Signature: `async fn(ctx: &Ctx, entity: &Entity<S>, cmd: C) -> crate::Result<Entity<S>>`
    ///
    /// Each call prepends a new `HandlerList<C, H, Tail>` node, so the compiler
    /// tracks which commands are registered in the type.
    pub fn with_handler<C, H>(
        self,
        handler: H,
    ) -> ServiceBuilder<S, L, HandlerList<C, H, Handlers>>
    where
        C: Command,
    {
        ServiceBuilder {
            loader: self.loader,
            handlers: HandlerList {
                handler,
                tail: self.handlers,
                _cmd: PhantomData,
            },
            _state: PhantomData,
        }
    }

    /// Consume the builder and produce a `BuiltService`.
    ///
    /// `factory` is an `async fn() -> crate::Result<Ctx>` called once per
    /// `load` or `execute` invocation to produce a fresh context. The concrete
    /// future type is erased into a `Box<dyn Future>` so `BuiltService`'s type
    /// stays ergonomic.
    pub fn build<Ctx, F, Fut>(
        self,
        factory: F,
    ) -> BuiltService<Ctx, S, L, Factory<Ctx>, Handlers>
    where
        Ctx: Send + Sync + 'static,
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = crate::Result<Ctx>> + Send + 'static,
    {
        let boxed: Factory<Ctx> = Box::new(move || Box::pin(factory()));
        BuiltService {
            factory: boxed,
            loader: self.loader,
            handlers: self.handlers,
            _ctx: PhantomData,
            _state: PhantomData,
        }
    }
}
