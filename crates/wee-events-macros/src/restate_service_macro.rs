//! `restate_service!` macro — generates a typed Restate ingress client and
//! server-side dispatch helper.
//!
//! # Syntax
//!
//! ```rust,ignore
//! wee_events_restate::restate_service! {
//!     pub CounterService for Counter {
//!         handlers: [
//!             Increment => increment,
//!             Adjust    => adjust,
//!         ],
//!     }
//! }
//! ```
//!
//! # Generated items
//!
//! ## Client side (`CounterServiceClient`)
//! - `pub struct CounterServiceClient` — typed Restate ingress client
//! - `CounterServiceClient::new(ingress_url, service_name)` — constructor
//! - `CounterServiceClient::load(&self, id) -> impl Future<..>`
//! - `CounterServiceClient::execute<C>(&self, id, cmd) -> impl Future<..>`
//!   (requires `Self: Handles<C>` — satisfied only for registered commands)
//! - `impl Handles<Increment> for CounterServiceClient` for each command
//! - `impl TypedService<Counter> for CounterServiceClient`
//!
//! ## Server side (`CounterServiceServer`)
//! - `pub struct CounterServiceServer` — zero-size server dispatch helper
//! - `CounterServiceServer::dispatch_json(service, name, target, command)` — typed
//!   JSON dispatch; deserializes the payload into each registered command type
//!   and routes to the matching handler via `TypedService::execute`

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

syn::custom_keyword!(handlers);
syn::custom_keyword!(loader);

// ---------------------------------------------------------------------------
// AST types
// ---------------------------------------------------------------------------

/// A single handler entry: `CommandType => handler_fn`
struct HandlerEntry {
    command_type: Path,
    /// Handler function — unused for client generation but required for
    /// syntactic compatibility with the `service!` macro so the same
    /// definition block can be shared across both macros.
    #[allow(dead_code)]
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

/// Full macro input:
/// ```text
/// pub ServiceName for StateType {
///     // optional — used for server-side dispatch generation
///     loader: load_fn,
///     handlers: [
///         CommandA => handler_a,
///         CommandB => handler_b,
///     ],
/// }
/// ```
struct RestateServiceInput {
    vis: Visibility,
    name: Ident,
    state_type: Path,
    /// Optional loader function path for server-side dispatch generation.
    #[allow(dead_code)]
    loader_fn: Option<Path>,
    handler_entries: Vec<HandlerEntry>,
}

impl Parse for RestateServiceInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let vis: Visibility = input.parse()?;
        let name: Ident = input.parse()?;
        let _for: Token![for] = input.parse()?;
        let state_type: Path = input.parse()?;

        let body;
        braced!(body in input);

        // Optional: loader: <path>,
        let loader_fn = if body.peek(loader) {
            let _loader_kw: loader = body.parse()?;
            let _colon: Token![:] = body.parse()?;
            let path: Path = body.parse()?;
            let _ = body.parse::<Token![,]>();
            Some(path)
        } else {
            None
        };

        // handlers: [ ... ],
        let _handlers_kw: handlers = body.parse()?;
        let _colon: Token![:] = body.parse()?;
        let entries_buf;
        bracketed!(entries_buf in body);
        let entries: Punctuated<HandlerEntry, Token![,]> =
            entries_buf.parse_terminated(HandlerEntry::parse, Token![,])?;

        // optional trailing comma after the bracket
        let _ = body.parse::<Token![,]>();

        Ok(RestateServiceInput {
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
    match syn::parse::<RestateServiceInput>(input) {
        Ok(service) => generate(service).into(),
        Err(e) => e.to_compile_error().into(),
    }
}

fn generate(service: RestateServiceInput) -> TokenStream2 {
    let RestateServiceInput {
        vis,
        name,
        state_type,
        loader_fn: _loader_fn,
        handler_entries,
    } = service;

    let client_name = format_ident!("{}Client", name);
    let server_name = format_ident!("{}Server", name);

    let cmd_types: Vec<&Path> = handler_entries.iter().map(|e| &e.command_type).collect();

    // ServiceState impl — associates the client with its state type.
    // The associated type approach avoids E0446 when state_type is private:
    // associated type values in impl blocks may reference private types.
    let service_state_impl = quote! {
        impl wee_events::__private::ServiceState for #client_name {
            type State = #state_type;
        }
    };

    // Per-command DispatchCommand<C> impls — required by Handles<C> supertrait.
    // The dispatch logic serializes and sends to the Restate ingress HTTP API.
    let dispatch_command_impls: Vec<TokenStream2> = handler_entries
        .iter()
        .map(|entry| {
            let cmd = &entry.command_type;
            quote! {
                impl wee_events::__private::DispatchCommand<#cmd> for #client_name {
                    fn dispatch_command(
                        &self,
                        id: &wee_events::AggregateId,
                        cmd: #cmd,
                    ) -> impl ::std::future::Future<
                        Output = wee_events::Result<wee_events::Entity<#state_type>>,
                    > + ::std::marker::Send + '_ {
                        // Delegate to the inherent execute method which handles
                        // serialization and the HTTP call.
                        #client_name::execute(self, id, cmd)
                    }
                }
            }
        })
        .collect();

    // `impl Handles<Cmd>` for each registered command.
    // The DispatchCommand<C> supertrait is satisfied by the impl above.
    let handles_impls: Vec<TokenStream2> = handler_entries
        .iter()
        .map(|entry| {
            let cmd = &entry.command_type;
            quote! {
                impl wee_events::Handles<#cmd> for #client_name {}
            }
        })
        .collect();

    // TypedService impl — delegates load to the inherent method.
    // The execute method uses the default impl from TypedService which calls
    // DispatchCommand<C>::dispatch_command.
    let typed_service_impl = quote! {
        impl wee_events::TypedService<#state_type> for #client_name {
            fn load(
                &self,
                id: &wee_events::AggregateId,
            ) -> impl ::std::future::Future<
                Output = wee_events::Result<wee_events::Entity<#state_type>>,
            > + ::std::marker::Send + '_ {
                #client_name::load(self, id)
            }
            // execute uses the default impl from TypedService which calls DispatchCommand<C>
        }
    };

    // Server dispatch arms — check name first via the type-level `Command::NAME`
    // constant, then deserialize only the matching type. This matches the spec's
    // intent: discriminate by name, decode only the selected payload.
    let dispatch_arms: Vec<TokenStream2> = cmd_types
        .iter()
        .map(|cmd| {
            quote! {
                if name.as_str() == <#cmd as wee_events::Command>::NAME {
                    let cmd: #cmd = ::serde_json::from_value(command)?;
                    return service.execute(target, cmd).await;
                }
            }
        })
        .collect();

    // Server-side dispatch struct — a zero-size type that holds the generated
    // dispatch logic. Separate from the client to make roles explicit.
    let server_struct = quote! {
        /// Generated server-side dispatch helper for #name.
        ///
        /// Provides `dispatch_json` which routes a JSON-encoded command payload to
        /// the appropriate typed handler on a `TypedService` implementation.
        /// This is the server-side complement to `#client_name`.
        #vis struct #server_name;

        impl #server_name {
            /// Dispatch a JSON-encoded command to the appropriate typed handler.
            ///
            /// Checks the command name against each registered type's
            /// `Command::NAME` constant, then deserializes only the matching
            /// payload type. This is static name-first dispatch — no
            /// speculative deserialization.
            ///
            /// Returns `Error::Rejection` with code `"UNKNOWN_COMMAND"` when no
            /// registered command type claims the given name.
            ///
            /// # Note on command types
            ///
            /// Each command type in the `handlers` list must implement
            /// `serde::Deserialize` so the payload can be deserialized server-side.
            #vis async fn dispatch_json<Svc>(
                service: &Svc,
                name: &wee_events::CommandName,
                target: &wee_events::AggregateId,
                command: ::serde_json::Value,
            ) -> wee_events::Result<wee_events::Entity<#state_type>>
            where
                Svc: wee_events::TypedService<#state_type>
                    #(+ wee_events::Handles<#cmd_types>)*,
            {
                #(#dispatch_arms)*

                Err(wee_events::Error::Rejection(
                    wee_events::Rejection::new(
                        "UNKNOWN_COMMAND",
                        format!("unknown command: {}", name.as_str()),
                    )
                ))
            }
        }
    };

    quote! {
        /// Generated typed Restate ingress client.
        ///
        /// Call `load` to fetch the current entity state via the Restate loader
        /// service, and `execute` to dispatch a command via the Restate executor
        /// workflow. Both methods call the Restate ingress HTTP API.
        ///
        /// The `execute` method requires `Self: Handles<C>` which is only
        /// satisfied for command types declared in the `restate_service!` block —
        /// unregistered commands produce a compile error.
        #vis struct #client_name {
            http: ::reqwest::Client,
            ingress_url: ::std::string::String,
            service_name: ::std::string::String,
        }

        impl #client_name {
            /// Create a new client pointing at the given Restate ingress URL for
            /// the named service.
            #vis fn new(
                ingress_url: impl ::std::convert::Into<::std::string::String>,
                service_name: impl ::std::convert::Into<::std::string::String>,
            ) -> Self {
                Self {
                    http: ::reqwest::Client::new(),
                    ingress_url: ingress_url.into(),
                    service_name: service_name.into(),
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
                let ingress_url = self.ingress_url.clone();
                let service_name = self.service_name.clone();
                let http = self.http.clone();
                async move {
                    wee_events_restate::generated::load::<#state_type>(
                        &http,
                        &ingress_url,
                        &service_name,
                        id,
                    )
                    .await
                }
            }

            /// Execute a typed command against the aggregate.
            ///
            /// The `Self: Handles<C>` bound ensures only commands declared
            /// in the `restate_service!` block can be dispatched.
            #vis fn execute<C>(
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
                let id = id.clone();
                let ingress_url = self.ingress_url.clone();
                let service_name = self.service_name.clone();
                let http = self.http.clone();
                // Serialize before the async block so the `Serialize` bound is
                // used synchronously. The async block only captures the already-
                // serialized value, avoiding lifetime/bound issues.
                let name = cmd.command_name();
                let command_result = ::serde_json::to_value(&cmd)
                    .map_err(wee_events::Error::Serialization);
                async move {
                    let command = command_result?;
                    wee_events_restate::generated::execute::<#state_type>(
                        &http,
                        &ingress_url,
                        &service_name,
                        id,
                        name,
                        command,
                    )
                    .await
                }
            }
        }

        #service_state_impl

        #(#dispatch_command_impls)*

        #(#handles_impls)*

        #typed_service_impl

        #server_struct
    }
}
