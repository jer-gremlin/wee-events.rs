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
// Idx computation
//
// Handlers are registered via `with_handler` which prepends to the HList.
// Input: [Cmd0, Cmd1, ..., CmdN-1]
// After build: HandlerList<CmdN-1, ..., HandlerList<Cmd0, _, EmptyHandlers>>
// So the last input command (index N-1) is the head → Here
//    the first input command (index 0) is deepest → There^(N-1)<Here>
//
// General: input index i → depth = N - 1 - i levels of There<...>
// ---------------------------------------------------------------------------

fn compute_idx(input_index: usize, total: usize) -> TokenStream2 {
    let depth = total - 1 - input_index;
    let mut idx = quote! { wee_events::Here };
    for _ in 0..depth {
        idx = quote! { wee_events::There<#idx> };
    }
    idx
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
        loader_fn: _loader_fn,
        handler_entries,
    } = service;

    let total = handler_entries.len();
    let cmd_types: Vec<&Path> = handler_entries.iter().map(|e| &e.command_type).collect();
    let _handler_fns: Vec<&Path> = handler_entries.iter().map(|e| &e.handler_fn).collect();

    // Generic type params for handlers: __H0, __H1, ...
    let handler_tparams: Vec<proc_macro2::Ident> = (0..total)
        .map(|i| format_ident!("__H{}", i))
        .collect();

    // Generic type param for loader
    let loader_tparam = format_ident!("__L");

    // Handles<Cmd, State> bounds for the return type
    let handles_bounds: Vec<TokenStream2> = cmd_types
        .iter()
        .map(|cmd| quote! { + wee_events::Handles<#cmd, #state_type> })
        .collect();

    // WHERE bounds for build(): loader and handlers constrain __Ctx
    let loader_bridge_bound = quote! {
        for<'a> &'a #loader_tparam: wee_events::LoaderBridge<'a, __Ctx, #state_type>,
    };
    let handler_bridge_bounds: Vec<TokenStream2> = cmd_types
        .iter()
        .zip(handler_tparams.iter())
        .map(|(cmd, hp)| {
            quote! {
                for<'a> &'a #hp: wee_events::HandlerBridge<'a, __Ctx, #state_type, #cmd>,
            }
        })
        .collect();

    // Extra WHERE bounds for the type params themselves
    let loader_tparam_bound = quote! {
        #loader_tparam: ::std::marker::Send + ::std::marker::Sync + 'static,
    };
    let handler_tparam_bounds: Vec<TokenStream2> = handler_tparams
        .iter()
        .map(|hp| {
            quote! {
                #hp: ::std::marker::Send + ::std::marker::Sync + 'static,
            }
        })
        .collect();

    // build() parameter list
    let loader_param = quote! { loader: #loader_tparam };
    let handler_params: Vec<TokenStream2> = handler_tparams
        .iter()
        .enumerate()
        .map(|(i, hp)| {
            let pname = format_ident!("handler_{}", i);
            quote! { #pname: #hp }
        })
        .collect();

    // with_loader + with_handler chain using the parameter names
    let with_loader_call = quote! { .with_loader(loader) };
    let with_handler_calls: Vec<TokenStream2> = cmd_types
        .iter()
        .enumerate()
        .map(|(i, cmd)| {
            let pname = format_ident!("handler_{}", i);
            quote! { .with_handler::<#cmd, _>(#pname) }
        })
        .collect();

    // All type params for build(): __Ctx, __F, __Fut, __L, __H0, __H1, ...
    let all_build_tparams: Vec<TokenStream2> = {
        let mut v = vec![
            quote! { __Ctx },
            quote! { __F },
            quote! { __Fut },
            quote! { #loader_tparam },
        ];
        for hp in &handler_tparams {
            v.push(quote! { #hp });
        }
        v
    };

    // Per-command DispatchCommand impls on the concrete BuiltService type.
    // These are generic over __Ctx, __L, __F, __H — verified at the call site
    // of execute() where the concrete types are known.
    let dispatch_impls: Vec<TokenStream2> = cmd_types
        .iter()
        .enumerate()
        .map(|(i, cmd)| {
            let idx = compute_idx(i, total);
            quote! {
                impl<__Ctx, __L, __F, __H> wee_events::__private::DispatchCommand<#cmd, #state_type>
                    for wee_events::BuiltService<__Ctx, #state_type, __L, __F, __H>
                where
                    __Ctx: ::std::marker::Send + ::std::marker::Sync + 'static,
                    __L: ::std::marker::Send + ::std::marker::Sync + 'static,
                    __F: ::std::marker::Send + ::std::marker::Sync + 'static,
                    __H: ::std::marker::Send + ::std::marker::Sync + 'static,
                    for<'a> &'a __L: wee_events::LoaderBridge<'a, __Ctx, #state_type>,
                    for<'a> &'a __F: wee_events::FactoryBridge<'a, __Ctx>,
                    __H: wee_events::HandleCommand<#cmd, #idx, __Ctx, #state_type>,
                {
                    fn dispatch_command(
                        &self,
                        id: &wee_events::AggregateId,
                        cmd: #cmd,
                    ) -> impl ::std::future::Future<
                        Output = wee_events::Result<wee_events::Entity<#state_type>>,
                    > + ::std::marker::Send {
                        let id = id.clone();
                        async move {
                            let ctx = <&__F as wee_events::FactoryBridge<'_, __Ctx>>::call(&self.factory).await?;
                            let entity = <&__L as wee_events::LoaderBridge<'_, __Ctx, #state_type>>::call(&self.loader, &ctx, &id).await?;
                            self.handlers.handle(&ctx, &entity, cmd).await
                        }
                    }
                }

                impl<__Ctx, __L, __F, __H> wee_events::Handles<#cmd, #state_type>
                    for wee_events::BuiltService<__Ctx, #state_type, __L, __F, __H>
                where
                    wee_events::BuiltService<__Ctx, #state_type, __L, __F, __H>:
                        wee_events::__private::DispatchCommand<#cmd, #state_type>,
                {}
            }
        })
        .collect();

    quote! {
        /// Generated service namespace.
        ///
        /// Call `build(factory, loader, handler_0, ...)` to produce a typed service
        /// with fully static dispatch. All type parameters are inferred from the
        /// arguments — no type erasure, no `Box<dyn Any>`.
        ///
        /// For convenience with the default (baked-in) loader and handlers, use
        /// `build_default(factory)`.
        #vis struct #name;

        impl #name {
            /// Build a service with explicit loader and handler arguments.
            ///
            /// The factory, loader, and handlers must all agree on the context
            /// type `Ctx` (inferred from the arguments).
            ///
            /// Returns an opaque type implementing `TypedService<S>` and `Handles<C>`
            /// for each registered command. Dispatch is fully static.
            #vis fn build<#(#all_build_tparams,)*>(
                factory: __F,
                #loader_param,
                #(#handler_params,)*
            ) -> impl wee_events::TypedService<#state_type>
                     #(#handles_bounds)*
            where
                __Ctx: ::std::marker::Send + ::std::marker::Sync + 'static,
                __F: ::std::ops::Fn() -> __Fut
                    + ::std::marker::Send
                    + ::std::marker::Sync
                    + 'static,
                __Fut: ::std::future::Future<Output = wee_events::Result<__Ctx>>
                    + ::std::marker::Send
                    + 'static,
                for<'a> &'a __F: wee_events::FactoryBridge<'a, __Ctx>,
                #loader_tparam_bound
                #loader_bridge_bound
                #(#handler_tparam_bounds)*
                #(#handler_bridge_bounds)*
            {
                wee_events::ServiceBuilder::<#state_type>::new()
                    #with_loader_call
                    #(#with_handler_calls)*
                    .build(factory)
            }

        }

        #(#dispatch_impls)*
    }
}
