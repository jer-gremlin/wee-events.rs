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

/// A single handler entry: a bare function path, e.g. `increment` or
/// `crate::domain::increment`.
struct HandlerEntry {
    fn_path: Path,
}

impl Parse for HandlerEntry {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let fn_path: Path = input.parse()?;
        Ok(HandlerEntry { fn_path })
    }
}

/// The full macro input:
/// ```text
/// pub ServiceName for StateType {
///     loader: load_fn,
///     handlers: [handler_a, handler_b],
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
// Identifier synthesis helpers
// ---------------------------------------------------------------------------

/// Given a function path `foo::bar::baz`, produce the spec struct path
/// `foo::bar::baz_Spec` by appending `_Spec` to the last segment ident.
fn spec_path(fn_path: &Path) -> Path {
    let mut spec = fn_path.clone();
    let last = spec.segments.last_mut().expect("path must have at least one segment");
    last.ident = format_ident!("{}_Spec", last.ident);
    spec
}

/// Given a function path `foo::bar::baz`, produce the `__Requires` trait path
/// `foo::bar::__baz_Requires` by prepending `__` and appending `_Requires` to the
/// last segment ident.
fn requires_path(fn_path: &Path) -> Path {
    let mut req = fn_path.clone();
    let last = req.segments.last_mut().expect("path must have at least one segment");
    last.ident = format_ident!("__{}_Requires", last.ident);
    req
}

// ---------------------------------------------------------------------------
// Idx computation
//
// Handlers are registered via `with_handler` which prepends to the HList.
// Input: [handler_0, handler_1, ..., handler_{N-1}]
// After build: HandlerList<Cmd_{N-1}, ..., HandlerList<Cmd_0, _, EmptyHandlers>>
// So the last input handler (index N-1) is the head → Here
//    the first input handler (index 0) is deepest → There^(N-1)<Here>
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
        loader_fn,
        handler_entries,
    } = service;

    let total = handler_entries.len();

    // Derive spec and requires paths for the loader
    let loader_spec_path = spec_path(&loader_fn);
    let loader_requires_path = requires_path(&loader_fn);

    // Derive spec and requires paths for each handler
    let handler_spec_paths: Vec<Path> = handler_entries.iter().map(|e| spec_path(&e.fn_path)).collect();
    let handler_requires_paths: Vec<Path> = handler_entries.iter().map(|e| requires_path(&e.fn_path)).collect();
    let handler_fn_paths: Vec<&Path> = handler_entries.iter().map(|e| &e.fn_path).collect();

    // Name for the generated service-specific env trait: {Name}Env
    let env_trait_name = format_ident!("{}Env", name);

    // All __Requires traits combined (loader + handlers)
    let all_requires_paths: Vec<&Path> = std::iter::once(&loader_requires_path)
        .chain(handler_requires_paths.iter())
        .collect();

    // -----------------------------------------------------------------------
    // 1. Service-specific env trait
    // -----------------------------------------------------------------------

    let env_trait = quote! {
        /// Environment contract for #name.
        ///
        /// Union of all capability requirements declared by the loader and handlers.
        /// Any type implementing the required capability traits automatically
        /// satisfies this trait via the blanket impl.
        #[allow(non_camel_case_types)]
        #vis trait #env_trait_name:
            #(#all_requires_paths +)*
            ::std::marker::Send + ::std::marker::Sync + 'static {}

        impl<__T> #env_trait_name for __T
        where
            __T: #(#all_requires_paths +)*
                 ::std::marker::Send + ::std::marker::Sync + 'static {}
    };

    // -----------------------------------------------------------------------
    // 2. Handles<C> bounds for the portable() return type
    //    Using <{fn}_Spec as HandlerSpec>::Command projections.
    // -----------------------------------------------------------------------

    let handles_bounds: Vec<TokenStream2> = handler_spec_paths
        .iter()
        .map(|sp| {
            quote! {
                + wee_events::Handles<<#sp as wee_events::HandlerSpec>::Command>
            }
        })
        .collect();

    // -----------------------------------------------------------------------
    // 3. with_handler calls inside portable()
    // -----------------------------------------------------------------------

    let with_handler_calls: Vec<TokenStream2> = handler_spec_paths
        .iter()
        .zip(handler_fn_paths.iter())
        .map(|(sp, fn_path)| {
            quote! {
                .with_handler::<<#sp as wee_events::HandlerSpec>::Command, _>(#fn_path::<__R>)
            }
        })
        .collect();

    // -----------------------------------------------------------------------
    // 4. Namespace struct with portable() constructor
    // -----------------------------------------------------------------------

    let namespace_struct = quote! {
        /// Generated service namespace.
        ///
        /// Call `portable(factory)` to produce a portable in-process service
        /// with fully static dispatch. All type parameters are inferred from
        /// the factory — no type erasure, no `Box<dyn Any>`.
        #vis struct #name;

        impl #name {
            /// Build a portable (in-process) service using the given factory
            /// to construct the environment per-operation.
            ///
            /// The factory's return type must satisfy `#env_trait_name` —
            /// the compiler verifies this automatically.
            #vis fn portable<__R, __F, __Fut>(factory: __F)
                -> impl wee_events::TypedService<#state_type>
                       #(#handles_bounds)*
            where
                __R: #env_trait_name,
                __F: ::std::ops::Fn() -> __Fut
                    + ::std::marker::Send
                    + ::std::marker::Sync
                    + 'static,
                __Fut: ::std::future::Future<Output = wee_events::Result<__R>>
                    + ::std::marker::Send
                    + 'static,
            {
                wee_events::ServiceBuilder::<#state_type>::new()
                    .with_loader(#loader_fn::<__R>)
                    #(#with_handler_calls)*
                    .build(factory)
            }
        }
    };

    // -----------------------------------------------------------------------
    // 5. DispatchCommand + Handles + TypedService impls on BuiltService
    // -----------------------------------------------------------------------

    let dispatch_impls: Vec<TokenStream2> = handler_spec_paths
        .iter()
        .enumerate()
        .map(|(i, sp)| {
            let idx = compute_idx(i, total);
            quote! {
                impl<__R, __L, __F, __H> wee_events::__private::DispatchCommand<
                    <#sp as wee_events::HandlerSpec>::Command,
                >
                    for wee_events::BuiltService<__R, #state_type, __L, __F, __H>
                where
                    __R: #env_trait_name,
                    __L: ::std::marker::Send + ::std::marker::Sync + 'static,
                    __F: ::std::marker::Send + ::std::marker::Sync + 'static,
                    __H: ::std::marker::Send + ::std::marker::Sync + 'static,
                    for<'a> &'a __L: wee_events::LoaderBridge<'a, __R, #state_type>,
                    for<'a> &'a __F: wee_events::FactoryBridge<'a, __R>,
                    __H: wee_events::HandleCommand<
                        <#sp as wee_events::HandlerSpec>::Command,
                        #idx,
                        __R,
                        #state_type,
                    >,
                {
                    fn dispatch_command(
                        &self,
                        id: &wee_events::AggregateId,
                        cmd: <#sp as wee_events::HandlerSpec>::Command,
                    ) -> impl ::std::future::Future<
                        Output = wee_events::Result<wee_events::Entity<#state_type>>,
                    > + ::std::marker::Send {
                        let id = id.clone();
                        async move {
                            let ctx = <&__F as wee_events::FactoryBridge<'_, __R>>::call(&self.factory).await?;
                            let entity = <&__L as wee_events::LoaderBridge<'_, __R, #state_type>>::call(&self.loader, &ctx, &id).await?;
                            self.handlers.handle(&ctx, &entity, cmd).await
                        }
                    }
                }

                impl<__R, __L, __F, __H> wee_events::Handles<
                    <#sp as wee_events::HandlerSpec>::Command,
                >
                    for wee_events::BuiltService<__R, #state_type, __L, __F, __H>
                where
                    wee_events::BuiltService<__R, #state_type, __L, __F, __H>:
                        wee_events::__private::DispatchCommand<
                            <#sp as wee_events::HandlerSpec>::Command,
                        >,
                {}
            }
        })
        .collect();

    let typed_service_impl = quote! {
        impl<__R, __L, __F, __H> wee_events::TypedService<#state_type>
            for wee_events::BuiltService<__R, #state_type, __L, __F, __H>
        where
            __R: #env_trait_name,
            __L: ::std::marker::Send + ::std::marker::Sync + 'static,
            __F: ::std::marker::Send + ::std::marker::Sync + 'static,
            __H: ::std::marker::Send + ::std::marker::Sync + 'static,
            for<'a> &'a __L: wee_events::LoaderBridge<'a, __R, #state_type>,
            for<'a> &'a __F: wee_events::FactoryBridge<'a, __R>,
        {
            fn load(
                &self,
                id: &wee_events::AggregateId,
            ) -> impl ::std::future::Future<
                Output = wee_events::Result<wee_events::Entity<#state_type>>,
            > + ::std::marker::Send {
                let id = id.clone();
                async move {
                    let ctx = <&__F as wee_events::FactoryBridge<'_, __R>>::call(&self.factory).await?;
                    <&__L as wee_events::LoaderBridge<'_, __R, #state_type>>::call(&self.loader, &ctx, &id).await
                }
            }
        }
    };

    // -----------------------------------------------------------------------
    // 6. Suppress the loader_spec_path unused warning (it's referenced only
    //    for the env trait, not for dispatch). Actually it is used — never mind.
    // -----------------------------------------------------------------------

    // Silence unused import: reference loader spec in a doc comment / type alias.
    // Actually loader_spec_path is used in the env trait only indirectly via
    // loader_requires_path. We don't reference it in generated code, so suppress
    // the compiler lint with a type assertion hidden in a const.
    let loader_spec_assertion = quote! {
        const _: () = {
            fn _assert_loader_spec() {
                fn _check<T: wee_events::LoaderSpec>() {}
                _check::<#loader_spec_path>();
            }
        };
    };

    quote! {
        #env_trait

        #namespace_struct

        #(#dispatch_impls)*

        #typed_service_impl

        #loader_spec_assertion
    }
}
