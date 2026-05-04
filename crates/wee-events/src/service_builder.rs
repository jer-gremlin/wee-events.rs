#![allow(private_bounds, private_interfaces)]

//! Portable async-factory runtime for building typed services.
//!
//! Provides a `ServiceBuilder<S>` that accumulates a loader function and typed
//! handler functions. The `.build(factory)` method produces a `BuiltService`
//! that has `load` and `execute` methods. All dispatch is fully static —
//! no type erasure, no `Box<dyn Any>`, no `TypeId`.
//!
//! ## Handler signature
//!
//! ```ignore
//! async fn my_handler(ctx: &Ctx, entity: &Entity<S>, cmd: C) -> crate::Result<()>
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
//! let entity = service.execute::<Increment, _>(&id, Increment { amount: 2 }).await?;
//! ```

use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;

use crate::entity::Entity;
use crate::id::AggregateId;
use crate::Command;

// ---------------------------------------------------------------------------
// Erased future type alias — BoxFuture for lifetime management only, not type erasure
// ---------------------------------------------------------------------------

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[doc(hidden)]
pub enum HandlerOutcome<S> {
    Entity(Entity<S>),
    Reload,
}

#[doc(hidden)]
pub trait IntoHandlerOutcome<S> {
    fn into_handler_outcome(self) -> crate::Result<HandlerOutcome<S>>;
}

impl<S> IntoHandlerOutcome<S> for crate::Result<Entity<S>> {
    fn into_handler_outcome(self) -> crate::Result<HandlerOutcome<S>> {
        self.map(HandlerOutcome::Entity)
    }
}

impl<S> IntoHandlerOutcome<S> for crate::Result<()> {
    fn into_handler_outcome(self) -> crate::Result<HandlerOutcome<S>> {
        self.map(|()| HandlerOutcome::Reload)
    }
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
    ) -> BoxFuture<'a, crate::Result<HandlerOutcome<S>>>;
}

impl<'a, Ctx, S, C, F, Fut, Output> HandlerBridge<'a, Ctx, S, C> for &'a F
where
    Ctx: Sync + 'a,
    S: Sync + 'a,
    C: Send + 'a,
    F: Fn(&'a Ctx, &'a Entity<S>, C) -> Fut + Sync,
    Fut: Future<Output = Output> + Send + 'a,
    Output: IntoHandlerOutcome<S> + 'a,
{
    fn call(
        f: Self,
        ctx: &'a Ctx,
        entity: &'a Entity<S>,
        cmd: C,
    ) -> BoxFuture<'a, crate::Result<HandlerOutcome<S>>> {
        Box::pin(async move { f(ctx, entity, cmd).await.into_handler_outcome() })
    }
}

// ---------------------------------------------------------------------------
// LoaderBridge — same lifetime trick for the loader function.
// ---------------------------------------------------------------------------

#[doc(hidden)]
pub trait LoaderBridge<'a, Ctx: 'a, S: 'a>: Sized {
    fn call(f: Self, ctx: &'a Ctx, id: &'a AggregateId) -> BoxFuture<'a, crate::Result<Entity<S>>>;
}

impl<'a, Ctx, S, F, Fut> LoaderBridge<'a, Ctx, S> for &'a F
where
    Ctx: 'a,
    S: 'a,
    F: Fn(&'a Ctx, &'a AggregateId) -> Fut,
    Fut: Future<Output = crate::Result<Entity<S>>> + Send + 'a,
{
    fn call(f: Self, ctx: &'a Ctx, id: &'a AggregateId) -> BoxFuture<'a, crate::Result<Entity<S>>> {
        Box::pin(f(ctx, id))
    }
}

// ---------------------------------------------------------------------------
// FactoryBridge — same lifetime trick for the factory function.
// ---------------------------------------------------------------------------

/// Lifetime-polymorphic bridge for factory functions.
///
/// Used by macro-generated `DispatchCommand` impls and `TypedService::load`
/// to call the factory without boxing the concrete closure type.
#[doc(hidden)]
pub trait FactoryBridge<'a, Ctx>: Sized {
    fn call(f: Self) -> BoxFuture<'a, crate::Result<Ctx>>;
}

impl<'a, Ctx, F, Fut> FactoryBridge<'a, Ctx> for &'a F
where
    F: Fn() -> Fut,
    Fut: Future<Output = crate::Result<Ctx>> + Send + 'a,
{
    fn call(f: Self) -> BoxFuture<'a, crate::Result<Ctx>> {
        Box::pin(f())
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
///
/// Fields are `pub` (doc-hidden) so adapter crates can implement their own
/// list-traversal traits (e.g. name-first dispatch for transport adapters)
/// without coupling the core crate to transport concerns.
#[doc(hidden)]
pub struct HandlerList<C, H, Tail> {
    #[doc(hidden)]
    pub handler: H,
    #[doc(hidden)]
    pub tail: Tail,
    #[doc(hidden)]
    pub _cmd: PhantomData<fn(C)>,
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
    ) -> BoxFuture<'a, crate::Result<HandlerOutcome<S>>>;
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
    ) -> BoxFuture<'a, crate::Result<HandlerOutcome<S>>> {
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
    ) -> BoxFuture<'a, crate::Result<HandlerOutcome<S>>> {
        self.tail.handle(ctx, entity, cmd)
    }
}

// ---------------------------------------------------------------------------
// BuiltService
// ---------------------------------------------------------------------------

/// A compiled service produced by `ServiceBuilder::build`.
///
/// - `load` is an inherent async method.
/// - `execute<C, Idx>` is an inherent method for direct HList dispatch.
/// - `TypedService<S>` is implemented via blanket impl.
/// - `DispatchCommand<C>` (and thus `Handles<C>`) impls are generated by
///   the `service!` macro for each registered command.
/// - `Send + Sync` when `L`, `F`, and `Handlers` are `Send + Sync`.
pub struct BuiltService<Ctx, S, L, F, Handlers> {
    pub factory: F,
    pub loader: L,
    pub handlers: Handlers,
    pub _ctx: PhantomData<fn() -> Ctx>,
    pub _state: PhantomData<fn() -> S>,
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

impl<Ctx, S, L, F, Handlers> BuiltService<Ctx, S, L, F, Handlers>
where
    Ctx: Send + Sync + 'static,
    S: Send + Sync + 'static,
    L: Send + Sync + 'static,
    F: Send + Sync + 'static,
    Handlers: Send + Sync + 'static,
    for<'a> &'a L: LoaderBridge<'a, Ctx, S>,
    for<'a> &'a F: FactoryBridge<'a, Ctx>,
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
            let ctx = <&F as FactoryBridge<'_, Ctx>>::call(&self.factory).await?;
            <&L as LoaderBridge<'_, Ctx, S>>::call(&self.loader, &ctx, &id).await
        }
    }

    /// Execute a typed command against the aggregate via direct HList dispatch.
    ///
    /// The `Idx` type parameter is an internal HList selector inferred by
    /// the compiler — callers never specify it explicitly. This inherent method
    /// is the escape hatch for direct use without the `service!` macro.
    ///
    /// Macro-generated code uses `DispatchCommand<C>::dispatch_command` instead,
    /// which goes through `Handles<C>` for the public `TypedService::execute` path.
    pub fn execute<C, Idx>(
        &self,
        id: &AggregateId,
        cmd: C,
    ) -> impl Future<Output = crate::Result<Entity<S>>> + Send + '_
    where
        C: Command + Send + 'static,
        Handlers: HandleCommand<C, Idx, Ctx, S>,
    {
        let id = id.clone();
        async move {
            let ctx = <&F as FactoryBridge<'_, Ctx>>::call(&self.factory).await?;
            let entity = <&L as LoaderBridge<'_, Ctx, S>>::call(&self.loader, &ctx, &id).await?;
            match self.handlers.handle(&ctx, &entity, cmd).await? {
                HandlerOutcome::Entity(entity) => Ok(entity),
                HandlerOutcome::Reload => {
                    <&L as LoaderBridge<'_, Ctx, S>>::call(&self.loader, &ctx, &id).await
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// TypedService blanket impl for BuiltService
// ---------------------------------------------------------------------------

impl<Ctx, S, L, F, Handlers> crate::service::__private::ServiceState
    for BuiltService<Ctx, S, L, F, Handlers>
where
    S: Send + Sync,
    L: Send + Sync,
    F: Send + Sync,
    Handlers: Send + Sync,
{
    type State = S;
}

// Note: TypedService<S> is NOT blanket-implemented on BuiltService here.
// The `service!` macro generates the TypedService impl per service because
// the `load` body requires `FactoryBridge` + `LoaderBridge` bounds that
// depend on the concrete Ctx inferred from the factory closure. A blanket
// impl with generic Ctx cannot satisfy `impl Trait` return type checking
// when the function bakes in a concrete loader.

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
    /// Signature: `async fn(ctx: &Ctx, entity: &Entity<S>, cmd: C) -> crate::Result<()>`
    ///
    /// Each call prepends a new `HandlerList<C, H, Tail>` node, so the compiler
    /// tracks which commands are registered in the type.
    pub fn with_handler<C, H>(self, handler: H) -> ServiceBuilder<S, L, HandlerList<C, H, Handlers>>
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
    /// factory type is preserved — no type erasure.
    pub fn build<Ctx, F, Fut>(self, factory: F) -> BuiltService<Ctx, S, L, F, Handlers>
    where
        Ctx: Send + Sync + 'static,
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = crate::Result<Ctx>> + Send + 'static,
    {
        self.build_raw(factory)
    }

    /// Consume the builder and produce a `BuiltService` without enforcing a
    /// factory shape.
    ///
    /// Escape hatch for adapters whose factories have non-standard signatures
    /// (e.g. `Fn(&some_adapter::Context) -> Future`). The adapter crate is
    /// responsible for providing the bridge impl that matches the factory
    /// shape; the portable interpreter requires the `Fn() -> Future` shape
    /// enforced by `build`.
    #[doc(hidden)]
    pub fn build_raw<Ctx, F>(self, factory: F) -> BuiltService<Ctx, S, L, F, Handlers> {
        BuiltService {
            factory,
            loader: self.loader,
            handlers: self.handlers,
            _ctx: PhantomData,
            _state: PhantomData,
        }
    }
}
