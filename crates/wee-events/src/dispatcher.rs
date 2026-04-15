#![allow(private_bounds, private_interfaces)]

use std::collections::HashMap;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;

use crate::entity::Entity;
use crate::id::CommandName;
use crate::service::Rejection;
use crate::store::RawEvent;

/// Erased future returned by handlers.
type HandlerFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Vec<RawEvent>, Rejection>> + Send + 'a>>;

// ---------------------------------------------------------------------------
// HandlerBridge: the lifetime-polymorphic glue layer.
//
// Rust's type system cannot express `F: for<'a> Fn(&'a Ctx, ...) -> Fut<'a>`
// because `Fut` is a single monomorphic type — it cannot vary with `'a`.
// The workaround is a helper trait whose `call` method takes an explicit `'a`,
// so the returned future is `impl Future + 'a`.  Implementing this trait for
// `&'a F` (a reference to the handler) ties the lifetime to the call site.
// ---------------------------------------------------------------------------

trait HandlerBridge<'a, Ctx: 'a, S: 'a, C>: Sized {
    fn call(f: Self, ctx: &'a Ctx, entity: &'a Entity<S>, cmd: C) -> HandlerFuture<'a>;
}

impl<'a, Ctx, S, C, F, Fut> HandlerBridge<'a, Ctx, S, C> for &'a F
where
    Ctx: 'a,
    S: 'a,
    F: Fn(&'a Ctx, &'a Entity<S>, C) -> Fut,
    Fut: Future<Output = Result<Vec<RawEvent>, Rejection>> + Send + 'a,
{
    fn call(f: Self, ctx: &'a Ctx, entity: &'a Entity<S>, cmd: C) -> HandlerFuture<'a> {
        Box::pin(f(ctx, entity, cmd))
    }
}

// ---------------------------------------------------------------------------
// Object-safe erasure layer
// ---------------------------------------------------------------------------

/// Object-safe handler that has already erased the command type `C`.
trait ErasedHandler<Ctx, S>: Send + Sync {
    fn handle<'a>(
        &'a self,
        ctx: &'a Ctx,
        entity: &'a Entity<S>,
        command: serde_json::Value,
    ) -> HandlerFuture<'a>;
}

/// Wraps a typed handler `F` and the command type `C` together, performing JSON
/// deserialization before delegation.  `C` lives in `PhantomData` so the impl
/// block can use it without it being an unconstrained type parameter on `F`.
struct TypedHandler<C, F> {
    handler: F,
    _cmd: PhantomData<fn() -> C>,
}

impl<Ctx, S, C, F> ErasedHandler<Ctx, S> for TypedHandler<C, F>
where
    Ctx: Send + Sync + 'static,
    S: Send + Sync + 'static,
    C: serde::de::DeserializeOwned + Send + 'static,
    F: Send + Sync + 'static,
    for<'a> &'a F: HandlerBridge<'a, Ctx, S, C>,
{
    fn handle<'a>(
        &'a self,
        ctx: &'a Ctx,
        entity: &'a Entity<S>,
        command: serde_json::Value,
    ) -> HandlerFuture<'a> {
        match serde_json::from_value::<C>(command) {
            Err(e) => {
                let msg = e.to_string();
                Box::pin(async move { Err(Rejection::new("COMMAND_VALIDATION_ERROR", msg)) })
            }
            Ok(cmd) => {
                <&'a F as HandlerBridge<'a, Ctx, S, C>>::call(&self.handler, ctx, entity, cmd)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Routes named commands to typed async handlers with context injection.
///
/// Each handler is an `async fn(ctx: &Ctx, entity: &Entity<S>, cmd: C) -> Result<Vec<RawEvent>, Rejection>`
/// where `C` is deserialized from the incoming JSON payload.
///
/// The `Ctx` type parameter carries dependencies via capability traits —
/// each handler constrains `Ctx` with the traits it needs (e.g., `Ctx: HasPublisher + HasEmail`).
/// This mirrors the Effect-TS `R` environment pattern where `R1 & R2` accumulates requirements.
///
/// At the boundary, provide a concrete context struct implementing all required traits.
pub struct Dispatcher<Ctx, S> {
    handlers: HashMap<CommandName, Box<dyn ErasedHandler<Ctx, S>>>,
}

impl<Ctx, S> Dispatcher<Ctx, S>
where
    Ctx: Send + Sync + 'static,
    S: Send + Sync + 'static,
{
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    /// Register a typed async handler for a named command.
    ///
    /// `C` is inferred from the handler signature and deserialized from the
    /// incoming `serde_json::Value`. A deserialization failure produces
    /// `Rejection { code: "COMMAND_VALIDATION_ERROR" }`.
    ///
    /// # Handler signature
    ///
    /// ```ignore
    /// async fn my_handler(ctx: &Ctx, entity: &Entity<S>, cmd: MyCmd) -> Result<Vec<RawEvent>, Rejection>
    /// ```
    pub fn handler<C, F>(mut self, name: impl Into<CommandName>, handler: F) -> Self
    where
        C: serde::de::DeserializeOwned + Send + 'static,
        F: Send + Sync + 'static,
        TypedHandler<C, F>: ErasedHandler<Ctx, S>,
    {
        self.handlers.insert(
            name.into(),
            Box::new(TypedHandler {
                handler,
                _cmd: PhantomData,
            }),
        );
        self
    }

    pub async fn dispatch(
        &self,
        ctx: &Ctx,
        entity: &Entity<S>,
        name: &CommandName,
        command: serde_json::Value,
    ) -> Result<Vec<RawEvent>, Rejection> {
        let handler = self.handlers.get(name).ok_or_else(|| {
            Rejection::new(
                "HANDLER_NOT_FOUND",
                format!("no handler registered for command: {name}"),
            )
        })?;

        handler.handle(ctx, entity, command).await
    }
}

impl<Ctx: Send + Sync + 'static, S: Send + Sync + 'static> Default for Dispatcher<Ctx, S> {
    fn default() -> Self {
        Self::new()
    }
}
