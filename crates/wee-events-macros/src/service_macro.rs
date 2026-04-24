use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{
    braced, bracketed,
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
    Ident, LitStr, Path, Token, Visibility,
};

// ---------------------------------------------------------------------------
// Custom keywords
// ---------------------------------------------------------------------------

syn::custom_keyword!(loader);
syn::custom_keyword!(handlers);
syn::custom_keyword!(effects);
syn::custom_keyword!(any);
syn::custom_keyword!(predicate);

// ---------------------------------------------------------------------------
// AST types
// ---------------------------------------------------------------------------

/// A single handler entry: `<fn_path>` optionally followed by `as "wire_name"`.
struct HandlerEntry {
    fn_path: Path,
    #[allow(dead_code)]
    wire_name: Option<LitStr>,
}

impl Parse for HandlerEntry {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let fn_path: Path = input.parse()?;
        let wire_name: Option<LitStr> = if input.peek(Token![as]) {
            let _as: Token![as] = input.parse()?;
            Some(input.parse::<LitStr>()?)
        } else {
            None
        };
        Ok(HandlerEntry { fn_path, wire_name })
    }
}

/// A single effect entry: `<WorkflowIdent> on <filter>`.
#[allow(dead_code)]
struct EffectEntry {
    workflow_ident: Ident,
    filter: EffectFilterSpec,
}

#[allow(dead_code)]
enum EffectFilterSpec {
    /// `on any`
    All,
    /// `on [Cmd1, Cmd2, ...]` - command type paths.
    Commands(Vec<Path>),
    /// `on predicate(|n| ...)` - a closure expression evaluating on `&ExecuteNotification`.
    Predicate(syn::ExprClosure),
}

impl Parse for EffectEntry {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let workflow_ident: Ident = input.parse()?;
        let on_kw: Ident = input.parse()?;
        if on_kw != "on" {
            return Err(syn::Error::new(on_kw.span(), "expected `on`"));
        }

        let filter = if input.peek(any) {
            let _: any = input.parse()?;
            EffectFilterSpec::All
        } else if input.peek(predicate) {
            let _: predicate = input.parse()?;
            let inner;
            syn::parenthesized!(inner in input);
            let closure: syn::ExprClosure = inner.parse()?;
            EffectFilterSpec::Predicate(closure)
        } else if input.peek(syn::token::Bracket) {
            let buf;
            bracketed!(buf in input);
            let cmds: Punctuated<Path, Token![,]> = buf.parse_terminated(Path::parse, Token![,])?;
            EffectFilterSpec::Commands(cmds.into_iter().collect())
        } else {
            return Err(input.error("expected `any`, `predicate(...)`, or `[Cmd, ...]` after `on`"));
        };

        Ok(EffectEntry {
            workflow_ident,
            filter,
        })
    }
}

/// The loader entry mirrors a handler entry but is parsed inline (no braces).
struct LoaderEntry {
    fn_path: Path,
    #[allow(dead_code)]
    wire_name: Option<LitStr>,
}

impl LoaderEntry {
    fn parse_inline(input: ParseStream) -> syn::Result<Self> {
        let fn_path: Path = input.parse()?;
        let wire_name: Option<LitStr> = if input.peek(Token![as]) {
            let _as: Token![as] = input.parse()?;
            Some(input.parse::<LitStr>()?)
        } else {
            None
        };
        Ok(LoaderEntry { fn_path, wire_name })
    }
}

/// The two forms accepted by `service!`:
///
/// - **DefinitionOnly**: `pub Name("logical") for State [Cmd, ...]`
///   Emits only `ServiceDefinition` + `HasCommand<C>` impls.
///
/// - **Full**: `pub Name for State { loader: .., handlers: [..] }`
///   Emits the env trait, portable constructor, and dispatch impls.
enum ServiceInput {
    DefinitionOnly(DefinitionOnlyInput),
    Full(FullServiceInput),
}

impl Parse for ServiceInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let vis: Visibility = input.parse()?;
        let name: Ident = input.parse()?;

        // Optional logical name literal: `("some-name")`
        let logical_name: Option<LitStr> = if input.peek(syn::token::Paren) {
            let inner;
            syn::parenthesized!(inner in input);
            Some(inner.parse::<LitStr>()?)
        } else {
            None
        };

        let _for: Token![for] = input.parse()?;
        let state_type: Path = input.parse()?;

        // Distinguish the two forms by what follows the state type.
        if input.peek(syn::token::Bracket) {
            // Definition-only: [ Cmd, ... ]
            let cmds_buf;
            bracketed!(cmds_buf in input);
            let commands: Punctuated<Path, Token![,]> =
                cmds_buf.parse_terminated(Path::parse, Token![,])?;

            let service_name = match logical_name {
                Some(lit) => lit.value(),
                None => to_snake_case(&name.to_string()),
            };

            Ok(ServiceInput::DefinitionOnly(DefinitionOnlyInput {
                vis,
                name,
                service_name,
                state_type,
                commands: commands.into_iter().collect(),
            }))
        } else {
            // Full form: { loader: .., handlers: [..] }
            let body;
            braced!(body in input);

            // loader: <path> [as "wire"] ,
            let _loader_kw: loader = body.parse()?;
            let _colon: Token![:] = body.parse()?;
            let loader_entry = LoaderEntry::parse_inline(&body)?;
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

            let effect_entries: Vec<EffectEntry> = if body.peek(effects) {
                let _: effects = body.parse()?;
                let _: Token![:] = body.parse()?;
                let eff_buf;
                bracketed!(eff_buf in body);
                let entries: Punctuated<EffectEntry, Token![,]> =
                    eff_buf.parse_terminated(EffectEntry::parse, Token![,])?;
                let _ = body.parse::<Token![,]>();
                entries.into_iter().collect()
            } else {
                Vec::new()
            };

            if !body.is_empty() {
                return Err(body.error("unexpected token in service body"));
            }

            let service_name = match logical_name {
                Some(lit) => lit.value(),
                None => to_snake_case(&name.to_string()),
            };

            Ok(ServiceInput::Full(FullServiceInput {
                vis,
                name,
                service_name,
                state_type,
                loader_entry,
                handler_entries: entries.into_iter().collect(),
                effect_entries,
            }))
        }
    }
}

/// Input for the definition-only form.
struct DefinitionOnlyInput {
    vis: Visibility,
    name: Ident,
    service_name: String,
    state_type: Path,
    commands: Vec<Path>,
}

/// Input for the full form (original struct, renamed for clarity).
struct FullServiceInput {
    vis: Visibility,
    name: Ident,
    service_name: String,
    state_type: Path,
    loader_entry: LoaderEntry,
    handler_entries: Vec<HandlerEntry>,
    effect_entries: Vec<EffectEntry>,
}

// ---------------------------------------------------------------------------
// snake_case helper
//
// Rule: insert `_` before each uppercase character that follows a lowercase
// character or digit, then lowercase everything.
// Examples: CounterService → counter_service, HTTPService → h_t_t_p_service
// (simple per-uppercase-transition rule; no acronym special-casing)
// ---------------------------------------------------------------------------

fn to_snake_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    let mut prev_lower = false;
    for ch in s.chars() {
        if ch.is_uppercase() {
            if prev_lower {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
            prev_lower = false;
        } else {
            out.push(ch);
            prev_lower = ch.is_lowercase() || ch.is_ascii_digit();
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Identifier synthesis helpers
// ---------------------------------------------------------------------------

/// Given a function path `foo::bar::baz`, produce the spec struct path
/// `foo::bar::baz_Spec` by appending `_Spec` to the last segment ident.
fn spec_path(fn_path: &Path) -> Path {
    let mut spec = fn_path.clone();
    let last = spec
        .segments
        .last_mut()
        .expect("path must have at least one segment");
    last.ident = format_ident!("{}_Spec", last.ident);
    spec
}

/// Given a function path `foo::bar::baz`, produce the `__Requires` trait path
/// `foo::bar::__baz_Requires` by prepending `__` and appending `_Requires` to the
/// last segment ident.
fn requires_path(fn_path: &Path) -> Path {
    let mut req = fn_path.clone();
    let last = req
        .segments
        .last_mut()
        .expect("path must have at least one segment");
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
        Ok(ServiceInput::DefinitionOnly(defn)) => generate_definition_only(defn).into(),
        Ok(ServiceInput::Full(full)) => generate_full(full).into(),
        Err(e) => e.to_compile_error().into(),
    }
}

// ---------------------------------------------------------------------------
// Definition-only emission
// ---------------------------------------------------------------------------

fn generate_definition_only(input: DefinitionOnlyInput) -> TokenStream2 {
    let DefinitionOnlyInput {
        vis,
        name,
        service_name,
        state_type,
        commands,
    } = input;

    let service_name_lit = LitStr::new(&service_name, proc_macro2::Span::call_site());

    quote! {
        /// Service definition for #name.
        ///
        /// State: `#state_type`
        #vis struct #name;

        impl ::wee_events::ServiceDefinition for #name {
            type State = #state_type;
            const SERVICE_NAME: &'static str = #service_name_lit;
        }

        #( impl ::wee_events::HasCommand<#commands> for #name {} )*

        impl #name {
            pub fn restate_client(
                ingress: impl Into<String>,
            ) -> ::wee_events_restate::RestateClient<Self> {
                ::wee_events_restate::RestateClient::new(ingress)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Full-form emission (original logic, refactored into its own function)
// ---------------------------------------------------------------------------

fn generate_full(service: FullServiceInput) -> TokenStream2 {
    let FullServiceInput {
        vis,
        name,
        service_name,
        state_type,
        loader_entry,
        handler_entries,
        effect_entries,
    } = service;
    let _ = &effect_entries;

    // Effective Restate method name for the loader.
    let loader_wire_name = loader_entry
        .wire_name
        .as_ref()
        .map(|l| l.value())
        .unwrap_or_else(|| "load".to_string());

    // Effective Restate method name for each handler, in input order.
    let handler_wire_names: Vec<String> = handler_entries
        .iter()
        .map(|e| match &e.wire_name {
            Some(lit) => lit.value(),
            None => to_snake_case(
                &e.fn_path
                    .segments
                    .last()
                    .expect("fn path has at least one segment")
                    .ident
                    .to_string(),
            ),
        })
        .collect();

    // Collision check: loader name must not collide with any handler name.
    let mut seen = std::collections::HashSet::new();
    seen.insert(loader_wire_name.clone());
    for (entry, wire) in handler_entries.iter().zip(handler_wire_names.iter()) {
        if !seen.insert(wire.clone()) {
            return syn::Error::new_spanned(
                &entry.fn_path,
                format!(
                    "wire name `{wire}` collides with another handler or with the reserved loader name `{loader}`. \
                     Use `{fn} as \"…\"` to pick a different wire name.",
                    wire = wire,
                    loader = loader_wire_name,
                    fn = entry
                        .fn_path
                        .segments
                        .last()
                        .map(|s| s.ident.to_string())
                        .unwrap_or_default(),
                ),
            )
            .to_compile_error();
        }
    }

    let loader_fn_path = &loader_entry.fn_path;
    let total = handler_entries.len();

    // Derive spec and requires paths for the loader
    let loader_spec_path = spec_path(loader_fn_path);
    let loader_requires_path = requires_path(loader_fn_path);

    // Derive spec and requires paths for each handler
    let handler_spec_paths: Vec<Path> = handler_entries
        .iter()
        .map(|e| spec_path(&e.fn_path))
        .collect();
    let handler_requires_paths: Vec<Path> = handler_entries
        .iter()
        .map(|e| requires_path(&e.fn_path))
        .collect();
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
                    .with_loader(#loader_fn_path::<__R>)
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

    // -----------------------------------------------------------------------
    // 7. ServiceDefinition + HasCommand<C> impls (definition traits)
    // -----------------------------------------------------------------------

    let service_name_lit = LitStr::new(&service_name, proc_macro2::Span::call_site());

    let definition_impls = quote! {
        impl ::wee_events::ServiceDefinition for #name {
            type State = #state_type;
            const SERVICE_NAME: &'static str = #service_name_lit;
        }

        #(
            impl ::wee_events::HasCommand<<#handler_spec_paths as ::wee_events::HandlerSpec>::Command>
                for #name {}
        )*

        impl #name {
            pub fn restate_client(
                ingress: impl Into<String>,
            ) -> ::wee_events_restate::RestateClient<Self> {
                ::wee_events_restate::RestateClient::new(ingress)
            }
        }
    };

    quote! {
        #env_trait

        #namespace_struct

        #(#dispatch_impls)*

        #typed_service_impl

        #loader_spec_assertion

        #definition_impls
    }
}
