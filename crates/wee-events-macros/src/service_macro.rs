use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
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

    // Per-command field names: `handler_0`, `handler_1`, ...
    let field_names: Vec<Ident> = handler_entries
        .iter()
        .enumerate()
        .map(|(i, _)| format_ident!("handler_{}", i))
        .collect();

    let cmd_types: Vec<&Path> = handler_entries.iter().map(|e| &e.command_type).collect();
    let handler_fns: Vec<&Path> = handler_entries.iter().map(|e| &e.handler_fn).collect();

    // Struct field declarations: `handler_0: wee_events::ErasedHandler<S>, ...`
    let field_decls: Vec<TokenStream2> = field_names
        .iter()
        .map(|field| {
            quote! {
                #field: wee_events::ErasedHandler<#state_type>,
            }
        })
        .collect();

    // Field initializers in `build()`:
    // `handler_0: wee_events::erase_handler::<_, S, CmdType, _>(handler_fn), ...`
    let field_inits: Vec<TokenStream2> = field_names
        .iter()
        .zip(cmd_types.iter())
        .zip(handler_fns.iter())
        .map(|((field, cmd), handler)| {
            quote! {
                #field: wee_events::erase_handler::<_, #state_type, #cmd, _>(#handler),
            }
        })
        .collect();

    // `impl Handles<Cmd>` for each registered command — no Idx on the public trait.
    let handles_impls: Vec<TokenStream2> = cmd_types
        .iter()
        .map(|cmd| {
            quote! {
                impl wee_events::Handles<#cmd> for #name {}
            }
        })
        .collect();

    // Static TypeId if-chain for `execute`: one branch per command.
    // Each branch downcasts cmd to the concrete type and calls the stored
    // ErasedHandler for that command. Because the chain is generated at
    // compile time with fixed types, there is no runtime data-structure scan.
    let dispatch_arms: Vec<TokenStream2> = field_names
        .iter()
        .zip(cmd_types.iter())
        .map(|(field, cmd)| {
            quote! {
                if __type_id == ::std::any::TypeId::of::<#cmd>() {
                    let concrete = *__cmd_any.downcast::<#cmd>().unwrap();
                    return (self.#field)(
                        ::std::sync::Arc::clone(&__ctx),
                        __entity,
                        ::std::boxed::Box::new(concrete),
                    )
                    .await;
                }
            }
        })
        .collect();

    // TypedService impl — load delegates directly; execute uses the static if-chain.
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

            fn execute<C>(
                &self,
                id: &wee_events::AggregateId,
                cmd: C,
            ) -> impl ::std::future::Future<
                Output = wee_events::Result<wee_events::Entity<#state_type>>,
            > + ::std::marker::Send + '_
            where
                C: wee_events::Command + ::std::marker::Send + 'static,
                Self: wee_events::Handles<C>,
            {
                #name::execute(self, id, cmd)
            }
        }
    };

    quote! {
        /// Generated service struct.
        ///
        /// The context type `Ctx` is erased at build time via `ErasedFactory` and
        /// `ErasedLoader`; concrete handlers are stored as individually-named fields
        /// (one per registered command) rather than a runtime `Vec`. The `execute`
        /// method dispatches via a generated static `TypeId` if-chain — no Vec scan.
        #vis struct #name {
            factory: wee_events::ErasedFactory,
            loader: wee_events::ErasedLoader<#state_type>,
            #(#field_decls)*
        }

        // SAFETY: All fields are `Send + Sync` by construction.
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
                Self {
                    factory: wee_events::erase_factory::<Ctx, _, _>(factory),
                    loader: wee_events::erase_loader::<_, #state_type, _>(#loader_fn),
                    #(#field_inits)*
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
            /// `Self: Handles<C>` is satisfied only for command types registered in the
            /// `service!` invocation — unregistered commands produce a compile error.
            ///
            /// Dispatch is via a generated static `TypeId` if-chain (one branch per
            /// registered command, inlined at codegen time). There is no runtime Vec or
            /// HashMap lookup.
            #vis fn execute<C>(
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
                Self: wee_events::Handles<C>,
            {
                let id = id.clone();
                let __type_id = ::std::any::TypeId::of::<C>();
                // Box the command once so it can be downcast to a concrete type
                // in the matching branch of the static if-chain below.
                let __cmd_any: ::std::boxed::Box<dyn ::std::any::Any + ::std::marker::Send + 'static> =
                    ::std::boxed::Box::new(cmd);

                async move {
                    let __ctx = (self.factory)().await?;
                    let __entity = (self.loader)(::std::sync::Arc::clone(&__ctx), id).await?;

                    // Generated static dispatch — one branch per registered command type.
                    // No Vec scan: the compiler emits N TypeId comparisons (all constants).
                    #(#dispatch_arms)*

                    unreachable!(
                        concat!(
                            stringify!(#name),
                            "::execute: no handler matched (Handles<C> guarantees C is \
                             registered — this is a bug in the service! macro)"
                        )
                    )
                }
            }
        }

        #(#handles_impls)*

        #typed_service_impl
    }
}
