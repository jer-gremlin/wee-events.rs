#![allow(private_bounds, private_interfaces)]

//! Portable async-factory runtime for building typed services.
//!
//! Provides a `ServiceBuilder<S>` that accumulates a loader function and typed
//! handler functions. The `.build(factory)` method produces a `BuiltService`
//! that has `load` and `execute` methods. All dispatch is fully static —
//! no type erasure, no `Box<dyn Any>`, no `TypeId`.
//!
//! ## Error model
//!
//! `BuiltService` carries two error generics:
//!
//! - `EL`: the loader's error type (also returned by `BuiltService::load`).
//! - `EH`: the handlers' error type (also returned by `BuiltService::execute`).
//!
//! Splitting them lets a service have a loader that fails with one error type
//! (commonly the store's error or a [`crate::ServiceError`] over it) and
//! handlers that fail with a richer service error. `EH: From<EL>` so loader
//! failures during `execute` flow into the handler error naturally; both must
//! be `From<crate::Error>` so factory failures (which produce `crate::Error`)
//! convert into either side.
//!
//! Defaults are `EL = crate::Error` and `EH = EL`, preserving the historical
//! single-`crate::Error` contract for callers that haven't migrated.
//!
//! ## Handler signature
//!
//! ```ignore
//! async fn my_handler(ctx: &Ctx, entity: &Entity<S>, cmd: C) -> Result<(), EH>
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
pub trait IntoHandlerOutcome<S, E> {
    fn into_handler_outcome(self) -> Result<HandlerOutcome<S>, E>;
}

impl<S, E> IntoHandlerOutcome<S, E> for Result<Entity<S>, E> {
    fn into_handler_outcome(self) -> Result<HandlerOutcome<S>, E> {
        self.map(HandlerOutcome::Entity)
    }
}

impl<S, E> IntoHandlerOutcome<S, E> for Result<(), E> {
    fn into_handler_outcome(self) -> Result<HandlerOutcome<S>, E> {
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
pub trait HandlerBridge<'a, Ctx: 'a, S: 'a, C, E>: Sized {
    fn call(
        f: Self,
        ctx: &'a Ctx,
        entity: &'a Entity<S>,
        cmd: C,
    ) -> BoxFuture<'a, Result<HandlerOutcome<S>, E>>;
}

impl<'a, Ctx, S, C, E, F, Fut, Output> HandlerBridge<'a, Ctx, S, C, E> for &'a F
where
    Ctx: Sync + 'a,
    S: Sync + 'a,
    C: Send + 'a,
    E: 'a,
    F: Fn(&'a Ctx, &'a Entity<S>, C) -> Fut + Sync,
    Fut: Future<Output = Output> + Send + 'a,
    Output: IntoHandlerOutcome<S, E> + 'a,
{
    fn call(
        f: Self,
        ctx: &'a Ctx,
        entity: &'a Entity<S>,
        cmd: C,
    ) -> BoxFuture<'a, Result<HandlerOutcome<S>, E>> {
        Box::pin(async move { f(ctx, entity, cmd).await.into_handler_outcome() })
    }
}

// ---------------------------------------------------------------------------
// LoaderBridge — same lifetime trick for the loader function.
// ---------------------------------------------------------------------------

#[doc(hidden)]
pub trait LoaderBridge<'a, Ctx: 'a, S: 'a, E>: Sized {
    fn call(f: Self, ctx: &'a Ctx, id: &'a AggregateId) -> BoxFuture<'a, Result<Entity<S>, E>>;
}

impl<'a, Ctx, S, E, F, Fut> LoaderBridge<'a, Ctx, S, E> for &'a F
where
    Ctx: 'a,
    S: 'a,
    E: 'a,
    F: Fn(&'a Ctx, &'a AggregateId) -> Fut,
    Fut: Future<Output = Result<Entity<S>, E>> + Send + 'a,
{
    fn call(f: Self, ctx: &'a Ctx, id: &'a AggregateId) -> BoxFuture<'a, Result<Entity<S>, E>> {
        Box::pin(f(ctx, id))
    }
}

// ---------------------------------------------------------------------------
// FactoryBridge — same lifetime trick for the factory function.
// ---------------------------------------------------------------------------

/// Lifetime-polymorphic bridge for factory functions.
///
/// Used by macro-generated `DispatchCommand` impls and `TypedService::load`
/// to call the factory without boxing the concrete closure type. Factories
/// remain pinned to `crate::Result<Ctx>` — context construction is an
/// infrastructure concern.
///
/// **Failure surface:** factory failures are converted into the loader's
/// `EL` (and the handler's `EH`) via the `From<crate::Error>` bound on each.
/// When `EL = ServiceError<E>`, that route lands in `ServiceError::Store(E::from(crate::Error))`,
/// so a context-construction failure is indistinguishable from a store-load
/// failure to the caller. This is intentional — the loader contract treats
/// "couldn't get a context" and "couldn't load the aggregate" as equally
/// fatal infrastructure conditions — but worth knowing if you're triaging
/// errors at the service boundary.
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
// Selector indices
// ---------------------------------------------------------------------------

/// The matching handler is at the head of this list.
#[doc(hidden)]
pub struct Here;

/// The matching handler is deeper in the tail at position `Idx`.
#[doc(hidden)]
pub struct There<Idx>(PhantomData<Idx>);

// ---------------------------------------------------------------------------
// HandleCommand<C, Idx, Ctx, S, E>
// ---------------------------------------------------------------------------

#[doc(hidden)]
pub trait HandleCommand<C, Idx, Ctx, S, E> {
    fn handle<'a>(
        &'a self,
        ctx: &'a Ctx,
        entity: &'a Entity<S>,
        cmd: C,
    ) -> BoxFuture<'a, Result<HandlerOutcome<S>, E>>;
}

impl<C, H, Tail, Ctx, S, E> HandleCommand<C, Here, Ctx, S, E> for HandlerList<C, H, Tail>
where
    Ctx: Send + Sync + 'static,
    S: Send + Sync + 'static,
    C: Send + 'static,
    E: 'static,
    H: Send + Sync + 'static,
    for<'a> &'a H: HandlerBridge<'a, Ctx, S, C, E>,
{
    fn handle<'a>(
        &'a self,
        ctx: &'a Ctx,
        entity: &'a Entity<S>,
        cmd: C,
    ) -> BoxFuture<'a, Result<HandlerOutcome<S>, E>> {
        <&'a H as HandlerBridge<'a, Ctx, S, C, E>>::call(&self.handler, ctx, entity, cmd)
    }
}

impl<C, Other, H, Tail, Idx, Ctx, S, E> HandleCommand<C, There<Idx>, Ctx, S, E>
    for HandlerList<Other, H, Tail>
where
    Tail: HandleCommand<C, Idx, Ctx, S, E>,
{
    fn handle<'a>(
        &'a self,
        ctx: &'a Ctx,
        entity: &'a Entity<S>,
        cmd: C,
    ) -> BoxFuture<'a, Result<HandlerOutcome<S>, E>> {
        self.tail.handle(ctx, entity, cmd)
    }
}

// ---------------------------------------------------------------------------
// BuiltService
// ---------------------------------------------------------------------------

/// A compiled service produced by `ServiceBuilder::build`.
///
/// - `EL` is the loader's error type (returned by `load`).
/// - `EH` is the handlers' error type (returned by `execute`); must be
///   `From<EL>` so loader failures flow into the handler error during
///   command dispatch.
/// - Both default to `crate::Error` so unmigrated callers get the historical
///   single-error contract.
pub struct BuiltService<Ctx, S, L, F, Handlers, EL = crate::Error, EH = EL> {
    pub factory: F,
    pub loader: L,
    pub handlers: Handlers,
    pub _ctx: PhantomData<fn() -> Ctx>,
    pub _state: PhantomData<fn() -> S>,
    pub _loader_error: PhantomData<fn() -> EL>,
    pub _handler_error: PhantomData<fn() -> EH>,
}

unsafe impl<Ctx, S, L, F, Handlers, EL, EH> Send for BuiltService<Ctx, S, L, F, Handlers, EL, EH>
where
    F: Send,
    L: Send,
    Handlers: Send,
{
}

unsafe impl<Ctx, S, L, F, Handlers, EL, EH> Sync for BuiltService<Ctx, S, L, F, Handlers, EL, EH>
where
    F: Sync,
    L: Sync,
    Handlers: Sync,
{
}

impl<Ctx, S, L, F, Handlers, EL, EH> BuiltService<Ctx, S, L, F, Handlers, EL, EH>
where
    Ctx: Send + Sync + 'static,
    S: Send + Sync + 'static,
    L: Send + Sync + 'static,
    F: Send + Sync + 'static,
    Handlers: Send + Sync + 'static,
    EL: From<crate::Error> + Send + Sync + 'static,
    EH: From<crate::Error> + From<EL> + Send + Sync + 'static,
    for<'a> &'a L: LoaderBridge<'a, Ctx, S, EL>,
    for<'a> &'a F: FactoryBridge<'a, Ctx>,
{
    /// Load the current entity state. Calls the factory then the loader.
    pub fn load(
        &self,
        id: &AggregateId,
    ) -> impl Future<Output = Result<Entity<S>, EL>> + Send + '_ {
        let id = id.clone();
        async move {
            let ctx = <&F as FactoryBridge<'_, Ctx>>::call(&self.factory)
                .await
                .map_err(EL::from)?;
            <&L as LoaderBridge<'_, Ctx, S, EL>>::call(&self.loader, &ctx, &id).await
        }
    }

    /// Execute a typed command via direct HList dispatch.
    pub fn execute<C, Idx>(
        &self,
        id: &AggregateId,
        cmd: C,
    ) -> impl Future<Output = Result<Entity<S>, EH>> + Send + '_
    where
        C: Command + Send + 'static,
        Handlers: HandleCommand<C, Idx, Ctx, S, EH>,
    {
        let id = id.clone();
        async move {
            let ctx = <&F as FactoryBridge<'_, Ctx>>::call(&self.factory)
                .await
                .map_err(EH::from)?;
            let entity = <&L as LoaderBridge<'_, Ctx, S, EL>>::call(&self.loader, &ctx, &id)
                .await
                .map_err(EH::from)?;
            match self.handlers.handle(&ctx, &entity, cmd).await? {
                HandlerOutcome::Entity(entity) => Ok(entity),
                HandlerOutcome::Reload => {
                    <&L as LoaderBridge<'_, Ctx, S, EL>>::call(&self.loader, &ctx, &id)
                        .await
                        .map_err(EH::from)
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// TypedService blanket impl helper
// ---------------------------------------------------------------------------

impl<Ctx, S, L, F, Handlers, EL, EH> crate::service::__private::ServiceState
    for BuiltService<Ctx, S, L, F, Handlers, EL, EH>
where
    S: Send + Sync,
    L: Send + Sync,
    F: Send + Sync,
    Handlers: Send + Sync,
    EL: Send + Sync,
    EH: Send + Sync,
{
    type State = S;
}

// ---------------------------------------------------------------------------
// ServiceBuilder
// ---------------------------------------------------------------------------

/// Builds a `BuiltService` by accumulating a loader and typed handler functions.
///
/// `EL` and `EH` (loader and handler error types) default to `crate::Error`
/// and are usually inferred from the registered functions. Use
/// [`ServiceBuilder::with_errors`] when an explicit pinning is needed.
pub struct ServiceBuilder<S, L = (), Handlers = EmptyHandlers, EL = crate::Error, EH = EL> {
    loader: L,
    handlers: Handlers,
    _state: PhantomData<fn() -> S>,
    _loader_error: PhantomData<fn() -> EL>,
    _handler_error: PhantomData<fn() -> EH>,
}

impl<S, EL, EH> ServiceBuilder<S, (), EmptyHandlers, EL, EH> {
    /// Create a new builder with no loader and no handlers registered.
    ///
    /// `EL` / `EH` are left free for inference from registered functions.
    pub fn new() -> Self {
        Self {
            loader: (),
            handlers: EmptyHandlers,
            _state: PhantomData,
            _loader_error: PhantomData,
            _handler_error: PhantomData,
        }
    }
}

impl<S, EL, EH> Default for ServiceBuilder<S, (), EmptyHandlers, EL, EH> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S, L, Handlers, EL, EH> ServiceBuilder<S, L, Handlers, EL, EH> {
    /// Pin the loader and handler error types explicitly. Rarely needed —
    /// inference from registered functions usually suffices.
    pub fn with_errors<EL2, EH2>(self) -> ServiceBuilder<S, L, Handlers, EL2, EH2> {
        ServiceBuilder {
            loader: self.loader,
            handlers: self.handlers,
            _state: PhantomData,
            _loader_error: PhantomData,
            _handler_error: PhantomData,
        }
    }

    /// Register the loader function.
    ///
    /// Signature: `async fn(ctx: &Ctx, id: &AggregateId) -> Result<Entity<S>, EL>`
    pub fn with_loader<NewL>(self, loader: NewL) -> ServiceBuilder<S, NewL, Handlers, EL, EH> {
        ServiceBuilder {
            loader,
            handlers: self.handlers,
            _state: PhantomData,
            _loader_error: PhantomData,
            _handler_error: PhantomData,
        }
    }

    /// Register a typed async handler for command `C`.
    ///
    /// Signature: `async fn(ctx: &Ctx, entity: &Entity<S>, cmd: C) -> Result<(), EH>`
    pub fn with_handler<C, H>(
        self,
        handler: H,
    ) -> ServiceBuilder<S, L, HandlerList<C, H, Handlers>, EL, EH>
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
            _loader_error: PhantomData,
            _handler_error: PhantomData,
        }
    }

    /// Consume the builder and produce a `BuiltService`.
    pub fn build<Ctx, F, Fut>(self, factory: F) -> BuiltService<Ctx, S, L, F, Handlers, EL, EH>
    where
        Ctx: Send + Sync + 'static,
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = crate::Result<Ctx>> + Send + 'static,
    {
        self.build_raw(factory)
    }

    /// Adapter escape hatch: produce a `BuiltService` without enforcing a
    /// factory shape. The caller's adapter must provide the bridge impl.
    #[doc(hidden)]
    pub fn build_raw<Ctx, F>(self, factory: F) -> BuiltService<Ctx, S, L, F, Handlers, EL, EH> {
        BuiltService {
            factory,
            loader: self.loader,
            handlers: self.handlers,
            _ctx: PhantomData,
            _state: PhantomData,
            _loader_error: PhantomData,
            _handler_error: PhantomData,
        }
    }
}
