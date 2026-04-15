//! `restate_service!` macro — generates a typed Restate ingress client.
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
//! - `pub struct CounterServiceClient` — typed Restate ingress client
//! - `CounterServiceClient::new(ingress_url, service_name)` — constructor
//! - `CounterServiceClient::load(&self, id) -> impl Future<..>`
//! - `CounterServiceClient::execute<C, Idx>(&self, id, cmd) -> impl Future<..>`
//!   (requires `Self: Handles<C, Idx>` — satisfied only for registered commands)
//! - `impl Handles<Increment, ()> for CounterServiceClient` for each command

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
        handler_entries,
    } = service;

    let client_name = format_ident!("{}Client", name);

    // `impl Handles<Cmd, ()>` for each registered command.
    let handles_impls: Vec<TokenStream2> = handler_entries
        .iter()
        .map(|entry| {
            let cmd = &entry.command_type;
            quote! {
                impl wee_events::Handles<#cmd, ()> for #client_name {}
            }
        })
        .collect();

    quote! {
        /// Generated typed Restate ingress client.
        ///
        /// Call `load` to fetch the current entity state via the Restate loader
        /// service, and `execute` to dispatch a command via the Restate executor
        /// workflow. Both methods call the Restate ingress HTTP API.
        ///
        /// The `execute` method requires `Self: Handles<C, Idx>` which is only
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
            /// The `Self: Handles<C, Idx>` bound ensures only commands declared
            /// in the `restate_service!` block can be dispatched.
            #vis fn execute<C, Idx>(
                &self,
                id: &wee_events::AggregateId,
                cmd: C,
            ) -> impl ::std::future::Future<
                Output = wee_events::Result<wee_events::Entity<#state_type>>,
            > + ::std::marker::Send + '_
            where
                C: wee_events::Command
                    + ::serde::Serialize
                    + ::std::marker::Send
                    + 'static,
                Self: wee_events::Handles<C, Idx>,
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

        #(#handles_impls)*

        // Note: `TypedService<S>` is not implemented for the generated client
        // because `execute` requires `C: Serialize` for JSON serialization over
        // the Restate ingress HTTP API, but `TypedService::execute` does not
        // include that bound. The inherent `load`/`execute` methods provide the
        // same compile-time `Handles<C>` safety. Functions accepting either a
        // local service or a Restate client should use a custom trait or generic
        // bound rather than `TypedService<S>`.
    }
}
