//! Implementation of the `#[handler(...)]` attribute macro.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
    Error, GenericArgument, ItemFn, Path, PathArguments, ReturnType, Token, Type,
};

// ---------------------------------------------------------------------------
// Attribute argument parsing
// ---------------------------------------------------------------------------

/// Parsed arguments for `#[handler(command = Path, requires(T, ...))]`.
struct HandlerArgs {
    command: Path,
    requires: Vec<Path>,
}

impl Parse for HandlerArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut command: Option<Path> = None;
        let mut requires: Vec<Path> = Vec::new();

        while !input.is_empty() {
            let lookahead = input.lookahead1();

            if lookahead.peek(syn::Ident) {
                let ident: syn::Ident = input.parse()?;

                if ident == "command" {
                    let _: Token![=] = input.parse()?;
                    let path: Path = input.parse()?;
                    command = Some(path);
                } else if ident == "requires" {
                    let content;
                    syn::parenthesized!(content in input);
                    let paths: Punctuated<Path, Token![,]> =
                        content.parse_terminated(Path::parse, Token![,])?;
                    requires = paths.into_iter().collect();
                } else {
                    return Err(Error::new_spanned(
                        ident,
                        "expected `command` or `requires`",
                    ));
                }
            } else {
                return Err(lookahead.error());
            }

            // Consume optional trailing comma between top-level args
            if input.peek(Token![,]) {
                let _: Token![,] = input.parse()?;
            }
        }

        let command = command.ok_or_else(|| {
            Error::new(
                proc_macro2::Span::call_site(),
                "`command = <Type>` is required for #[handler]",
            )
        })?;

        Ok(HandlerArgs { command, requires })
    }
}

// ---------------------------------------------------------------------------
// State extraction from return type
// ---------------------------------------------------------------------------

/// Extract the `State` from `wee_events::Result<Entity<State>>` or
/// `wee_events::Result<wee_events::Entity<State>>`.
fn extract_state_type(return_type: &ReturnType) -> syn::Result<Type> {
    let ReturnType::Type(_, box_ty) = return_type else {
        return Err(Error::new(
            proc_macro2::Span::call_site(),
            "handler must return `wee_events::Result<Entity<State>>`",
        ));
    };

    let Type::Path(type_path) = box_ty.as_ref() else {
        return Err(Error::new_spanned(
            box_ty.as_ref(),
            "handler must return `wee_events::Result<Entity<State>>`",
        ));
    };

    // Navigate the path segments to find Result<...>
    // Accept: Result<...>, wee_events::Result<...>
    let last_seg = type_path.path.segments.last().ok_or_else(|| {
        Error::new_spanned(
            box_ty.as_ref(),
            "handler must return `wee_events::Result<Entity<State>>`",
        )
    })?;

    if last_seg.ident != "Result" {
        return Err(Error::new_spanned(
            box_ty.as_ref(),
            "handler must return `wee_events::Result<Entity<State>>`",
        ));
    }

    // Extract the single generic argument of Result<T>
    let PathArguments::AngleBracketed(result_args) = &last_seg.arguments else {
        return Err(Error::new_spanned(
            box_ty.as_ref(),
            "handler must return `wee_events::Result<Entity<State>>`",
        ));
    };

    let entity_ty = result_args
        .args
        .iter()
        .find_map(|arg| {
            if let GenericArgument::Type(ty) = arg {
                Some(ty)
            } else {
                None
            }
        })
        .ok_or_else(|| {
            Error::new_spanned(
                box_ty.as_ref(),
                "handler must return `wee_events::Result<Entity<State>>`",
            )
        })?;

    // Now extract State from Entity<State>
    let Type::Path(entity_path) = entity_ty else {
        return Err(Error::new_spanned(
            entity_ty,
            "handler must return `wee_events::Result<Entity<State>>`",
        ));
    };

    let entity_seg = entity_path.path.segments.last().ok_or_else(|| {
        Error::new_spanned(
            entity_ty,
            "handler must return `wee_events::Result<Entity<State>>`",
        )
    })?;

    if entity_seg.ident != "Entity" {
        return Err(Error::new_spanned(
            entity_ty,
            "handler must return `wee_events::Result<Entity<State>>`",
        ));
    }

    let PathArguments::AngleBracketed(entity_args) = &entity_seg.arguments else {
        return Err(Error::new_spanned(
            entity_ty,
            "handler must return `wee_events::Result<Entity<State>>`",
        ));
    };

    let state_ty = entity_args
        .args
        .iter()
        .find_map(|arg| {
            if let GenericArgument::Type(ty) = arg {
                Some(ty.clone())
            } else {
                None
            }
        })
        .ok_or_else(|| {
            Error::new_spanned(
                entity_ty,
                "handler must return `wee_events::Result<Entity<State>>`",
            )
        })?;

    Ok(state_ty)
}

// ---------------------------------------------------------------------------
// Macro expansion
// ---------------------------------------------------------------------------

pub fn expand(args: TokenStream, input: TokenStream) -> TokenStream {
    let args = syn::parse_macro_input!(args as HandlerArgs);
    let func = syn::parse_macro_input!(input as ItemFn);

    match expand_inner(args, func) {
        Ok(ts) => ts.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

fn expand_inner(args: HandlerArgs, func: ItemFn) -> syn::Result<TokenStream2> {
    // Validate: function must have at least one generic type parameter
    if func.sig.generics.type_params().next().is_none() {
        return Err(Error::new_spanned(
            &func.sig,
            "#[handler] requires a generic type parameter for the context (e.g., `<R: MyTrait>`)",
        ));
    }

    let state_ty = extract_state_type(&func.sig.output)?;

    let fn_name = &func.sig.ident;
    let vis = &func.vis;
    let command_path = &args.command;
    let requires = &args.requires;

    // Spec struct name: {fn_name}_Spec
    let spec_name = syn::Ident::new(&format!("{}_Spec", fn_name), fn_name.span());

    // Composite requires trait name: __{fn_name}_Requires
    let requires_trait_name = syn::Ident::new(&format!("__{}_Requires", fn_name), fn_name.span());

    // Build the supertraits for the composite requires trait
    let requires_supertraits: TokenStream2 = if requires.is_empty() {
        quote! { ::std::marker::Send + ::std::marker::Sync }
    } else {
        let paths = requires.iter();
        quote! { #(#paths +)* ::std::marker::Send + ::std::marker::Sync }
    };

    // Build the where clause for the blanket impl
    let requires_where: TokenStream2 = if requires.is_empty() {
        quote! { T: ::std::marker::Send + ::std::marker::Sync + ?Sized }
    } else {
        let paths = requires.iter();
        quote! { T: #(#paths +)* ::std::marker::Send + ::std::marker::Sync + ?Sized }
    };

    Ok(quote! {
        #func

        #[allow(non_camel_case_types)]
        #[doc(hidden)]
        #vis struct #spec_name;

        impl wee_events::HandlerSpec for #spec_name {
            type Command = #command_path;
            type State = #state_ty;
        }

        #[allow(non_camel_case_types)]
        #[doc(hidden)]
        #vis trait #requires_trait_name: #requires_supertraits {}

        impl<T> #requires_trait_name for T
        where #requires_where {}
    })
}
