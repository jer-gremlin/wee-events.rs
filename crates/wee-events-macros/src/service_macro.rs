use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{
    braced, bracketed,
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
    Ident, Path, Token, Visibility,
};

// ---------------------------------------------------------------------------
// Custom keywords
// ---------------------------------------------------------------------------

syn::custom_keyword!(loader);
syn::custom_keyword!(handlers);

// ---------------------------------------------------------------------------
// AST types
// ---------------------------------------------------------------------------

/// A single handler entry: `CommandType => handler_fn`
struct HandlerEntry {
    command_type: Path,
    handler_fn: Path,
}

impl Parse for HandlerEntry {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let command_type: Path = input.parse()?;
        let _arrow: Token![=>] = input.parse()?;
        let handler_fn: Path = input.parse()?;
        Ok(HandlerEntry {
            command_type,
            handler_fn,
        })
    }
}

/// The full macro input:
/// ```text
/// pub ServiceName for StateType {
///     loader: load_fn,
///     handlers: [
///         CommandA => handler_a,
///         CommandB => handler_b,
///     ],
/// }
/// ```
struct ServiceInput {
    vis: Visibility,
    name: Ident,
    state_type: Path,
    loader_fn: Path,
    handler_entries: Vec<HandlerEntry>,
}

impl Parse for ServiceInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let vis: Visibility = input.parse()?;
        let name: Ident = input.parse()?;
        let _for: Token![for] = input.parse()?;
        let state_type: Path = input.parse()?;

        let body;
        braced!(body in input);

        // loader: <path>,
        let _loader_kw: loader = body.parse()?;
        let _colon: Token![:] = body.parse()?;
        let loader_fn: Path = body.parse()?;
        let _comma: Token![,] = body.parse()?;

        // handlers: [ ... ],
        let _handlers_kw: handlers = body.parse()?;
        let _colon2: Token![:] = body.parse()?;
        let entries_buf;
        bracketed!(entries_buf in body);
        let entries: Punctuated<HandlerEntry, Token![,]> =
            entries_buf.parse_terminated(HandlerEntry::parse, Token![,])?;

        // optional trailing comma after the bracket
        let _ = body.parse::<Token![,]>();

        Ok(ServiceInput {
            vis,
            name,
            state_type,
            loader_fn,
            handler_entries: entries.into_iter().collect(),
        })
    }
}

// ---------------------------------------------------------------------------
// Code generation
// ---------------------------------------------------------------------------

pub fn expand(input: TokenStream) -> TokenStream {
    match syn::parse::<ServiceInput>(input) {
        Ok(service) => generate(service).into(),
        Err(e) => e.to_compile_error().into(),
    }
}

fn generate(service: ServiceInput) -> TokenStream2 {
    let ServiceInput {
        vis,
        name,
        state_type,
        loader_fn,
        handler_entries,
    } = service;

    let cmd_types: Vec<&Path> = handler_entries.iter().map(|e| &e.command_type).collect();
    let handler_fns: Vec<&Path> = handler_entries.iter().map(|e| &e.handler_fn).collect();

    // `impl Handles<Cmd, ()>` for each registered command.
    let handles_impls: Vec<TokenStream2> = handler_entries
        .iter()
        .map(|entry| {
            let cmd = &entry.command_type;
            quote! {
                impl wee_events::Handles<#cmd, ()> for #name {}
            }
        })
        .collect();

    // TypedService impl — delegates to inherent execute/load.
    let typed_service_impl = quote! {
        impl wee_events::TypedService<#state_type> for #name {
            fn load(
                &self,
                id: &wee_events::AggregateId,
            ) -> impl ::std::future::Future<
                Output = wee_events::Result<wee_events::Entity<#state_type>>,
            > + ::std::marker::Send + '_ {
                #name::load(self, id)
            }

            fn execute<C, Idx>(
                &self,
                id: &wee_events::AggregateId,
                cmd: C,
            ) -> impl ::std::future::Future<
                Output = wee_events::Result<wee_events::Entity<#state_type>>,
            > + ::std::marker::Send + '_
            where
                C: wee_events::Command + ::std::marker::Send + 'static,
                Self: wee_events::Handles<C, Idx>,
            {
                #name::execute(self, id, cmd)
            }
        }
    };

    quote! {
        /// Generated service struct. All operations are fully type-erased
        /// so the struct carries no generic parameters. The concrete context
        /// type `Ctx` is erased at build time and restored via `Any` downcasting.
        ///
        /// Note: the `build` factory must produce the same context type `Ctx`
        /// that the loader and handler functions accept. A mismatch causes a
        /// runtime panic (a programmer error, not a domain error).
        #vis struct #name {
            factory: wee_events::ErasedFactory,
            loader: wee_events::ErasedLoader<#state_type>,
            handlers: ::std::vec::Vec<(
                ::std::any::TypeId,
                wee_events::ErasedHandler<#state_type>,
            )>,
        }

        // SAFETY: All fields are `Send + Sync` by construction — `ErasedFactory`,
        // `ErasedLoader<S>`, and `ErasedHandler<S>` are `Box<dyn ... + Send + Sync>`.
        // We use explicit impls because the `dyn Any` trait objects prevent auto-derivation.
        unsafe impl ::std::marker::Send for #name
        where
            wee_events::ErasedFactory: ::std::marker::Send,
            wee_events::ErasedLoader<#state_type>: ::std::marker::Send,
            wee_events::ErasedHandler<#state_type>: ::std::marker::Send,
        {}
        unsafe impl ::std::marker::Sync for #name
        where
            wee_events::ErasedFactory: ::std::marker::Sync,
            wee_events::ErasedLoader<#state_type>: ::std::marker::Sync,
            wee_events::ErasedHandler<#state_type>: ::std::marker::Sync,
        {}

        impl #name {
            /// Build a service instance from the given async context factory.
            ///
            /// `Ctx` is inferred from the factory closure and must match the
            /// context type expected by the loader and handler functions baked
            /// into this service.
            #vis fn build<Ctx, F, Fut>(factory: F) -> Self
            where
                Ctx: ::std::any::Any
                    + ::std::marker::Send
                    + ::std::marker::Sync
                    + 'static,
                F: ::std::ops::Fn() -> Fut
                    + ::std::marker::Send
                    + ::std::marker::Sync
                    + 'static,
                Fut: ::std::future::Future<Output = wee_events::Result<Ctx>>
                    + ::std::marker::Send
                    + 'static,
            {
                use ::std::any::TypeId;
                Self {
                    factory: wee_events::erase_factory::<Ctx, _, _>(factory),
                    loader: wee_events::erase_loader::<_, #state_type, _>(#loader_fn),
                    handlers: ::std::vec![
                        #((
                            TypeId::of::<#cmd_types>(),
                            wee_events::erase_handler::<_, #state_type, #cmd_types, _>(
                                #handler_fns,
                            ),
                        ),)*
                    ],
                }
            }

            /// Load the current entity state for the given aggregate.
            #vis fn load(
                &self,
                id: &wee_events::AggregateId,
            ) -> impl ::std::future::Future<
                Output = wee_events::Result<wee_events::Entity<#state_type>>,
            > + ::std::marker::Send + '_ {
                let id = id.clone();
                async move {
                    let ctx = (self.factory)().await?;
                    (self.loader)(ctx, id).await
                }
            }

            /// Execute a typed command against the aggregate.
            ///
            /// `Self: Handles<C, Idx>` is satisfied only for command types
            /// registered in the `service!` invocation — unregistered commands
            /// produce a compile error. The actual dispatch is via `TypeId`
            /// lookup at runtime (zero-overhead compared to dyn vtable).
            #vis fn execute<C, Idx>(
                &self,
                id: &wee_events::AggregateId,
                cmd: C,
            ) -> impl ::std::future::Future<
                Output = wee_events::Result<wee_events::Entity<#state_type>>,
            > + ::std::marker::Send + '_
            where
                C: wee_events::Command
                    + ::std::any::Any
                    + ::std::marker::Send
                    + 'static,
                Self: wee_events::Handles<C, Idx>,
            {
                let id = id.clone();
                let type_id = ::std::any::TypeId::of::<C>();
                let cmd_any: ::std::boxed::Box<
                    dyn ::std::any::Any + ::std::marker::Send + 'static,
                > = ::std::boxed::Box::new(cmd);

                async move {
                    let ctx = (self.factory)().await?;
                    // Arc::clone is cheap — shares the context between loader and handler
                    let entity = (self.loader)(::std::sync::Arc::clone(&ctx), id).await?;
                    let handler = self
                        .handlers
                        .iter()
                        .find(|(tid, _)| *tid == type_id)
                        .map(|(_, h)| h)
                        .expect(
                            concat!(
                                stringify!(#name),
                                "::execute: no handler for command \
                                 (Handles<C> was satisfied at compile time — \
                                 this indicates a bug in the service! macro)"
                            )
                        );
                    handler(ctx, entity, cmd_any).await
                }
            }
        }

        #(#handles_impls)*

        #typed_service_impl
    }
}
