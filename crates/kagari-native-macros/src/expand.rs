//! Expand authoring declarations into records and bindings, without duplicating ABI validation.
use crate::parse::{Item, Method, Module};
use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{
    Attribute, Error as SyntaxError, Expr, FnArg, GenericArgument, GenericParam, Generics, Ident,
    ItemTrait, ItemType, Lit, Meta, Pat, Path, PathArguments, Result as SyntaxResult, ReturnType,
    Signature, TraitBoundModifier, TraitItem, Type, TypeParamBound,
};

pub fn module(module: Module) -> SyntaxResult<TokenStream> {
    let runtime = &module.runtime;
    let builder = builder_ident();
    let path = path_names(&module.path)?;
    if path.len() < 2 {
        return Err(SyntaxError::new_spanned(
            &module.path,
            "module needs a package and module path",
        ));
    }
    let mut declarations = vec![];
    for item in module.items {
        declarations.push(match item {
            Item::Storage(item) => storage_declaration(item)?,
            Item::Trait(item) => trait_declaration(item, runtime)?,
            Item::Implementation {
                generics,
                contract,
                receiver,
                methods,
                bindings,
            } => impl_declaration(
                generics,
                contract.as_deref(),
                &receiver,
                &methods,
                &bindings,
                runtime,
            )?,
            Item::Function(method) => free_function(method, runtime)?,
        });
    }
    Ok(quote! {
        (|| -> ::std::result::Result<#runtime::NativeApi, #runtime::RuntimeError> {
            let mut #builder = #runtime::native_module::NativeModuleBuilder::new(&[#(#path),*])?;
            #(#declarations)*
            #builder.finish()
        })()
    })
}

fn storage_declaration(item: ItemType) -> SyntaxResult<TokenStream> {
    let names = generic_names(&item.generics)?;
    let valid = if let Type::Path(ty) = item.ty.as_ref() {
        if ty.qself.is_some() || ty.path.segments.len() != 1 {
            false
        } else if let Some(segment) = ty.path.segments.first() {
            if let PathArguments::AngleBracketed(args) = &segment.arguments {
                segment.ident == "native_array"
                    && names.len() == 1
                    && args.args.len() == 1
                    && matches!(args.args.first(), Some(GenericArgument::Type(Type::Path(arg)))
                        if arg.qself.is_none() && arg.path.is_ident(&names[0]))
            } else {
                false
            }
        } else {
            false
        }
    } else {
        false
    };
    if !valid {
        return Err(SyntaxError::new_spanned(
            &item.ty,
            "expected native_array<T> with the declared type parameter",
        ));
    }
    let name = item.ident.to_string();
    let doc = documentation(&item.attrs)?;
    let builder = builder_ident();
    Ok(quote! { #builder.array_type(#name, &[#(#names),*], #doc)?; })
}

fn trait_declaration(item: ItemTrait, runtime: &Path) -> SyntaxResult<TokenStream> {
    if item.unsafety.is_some() || item.auto_token.is_some() {
        return Err(SyntaxError::new_spanned(
            item.ident,
            "unsafe/auto native traits are unsupported",
        ));
    }
    let names = generic_names(&item.generics)?;
    let name = item.ident.to_string();
    let doc = documentation(&item.attrs)?;
    let builder = builder_ident();
    let parents = item
        .supertraits
        .iter()
        .map(|parent| {
            if let TypeParamBound::Trait(parent) = parent {
                if parent.lifetimes.is_some()
                    || !matches!(parent.modifier, TraitBoundModifier::None)
                {
                    return Err(SyntaxError::new_spanned(
                        parent,
                        "unsupported native supertrait bound",
                    ));
                }
                named_type(&parent.path, runtime)
            } else {
                Err(SyntaxError::new_spanned(
                    parent,
                    "expected a native supertrait",
                ))
            }
        })
        .collect::<SyntaxResult<Vec<_>>>()?;
    let methods = item
        .items
        .iter()
        .map(|method| {
            let TraitItem::Fn(method) = method else {
                return Err(SyntaxError::new_spanned(
                    method,
                    "only required methods are supported",
                ));
            };
            if method.default.is_some() || !method.sig.generics.params.is_empty() {
                return Err(SyntaxError::new_spanned(
                    method,
                    "required methods cannot have bodies or local generics",
                ));
            }
            method_record(&method.sig, &method.attrs, None, runtime)
        })
        .collect::<SyntaxResult<Vec<_>>>()?;
    Ok(
        quote! { #builder.required_trait(#name, &[#(#names),*], #doc, ::std::vec![#(#parents),*], ::std::vec![#(#methods),*])?; },
    )
}

fn impl_declaration(
    generics: Generics,
    contract: Option<&Type>,
    receiver: &Type,
    methods: &[Method],
    bindings: &[(Ident, Path)],
    runtime: &Path,
) -> SyntaxResult<TokenStream> {
    let names = generic_names(&generics)?;
    let builder = builder_ident();
    let receiver = ty(receiver, runtime)?;
    if let Some(contract) = contract {
        let contract = ty(contract, runtime)?;
        let bindings = bindings
            .iter()
            .map(|(name, factory)| {
                let name = name.to_string();
                let binding = binding(factory, runtime)?;
                Ok(quote! { (#name, #binding) })
            })
            .collect::<SyntaxResult<Vec<_>>>()?;
        Ok(
            quote! { #builder.trait_impl(&[#(#names),*], #receiver, #contract, ::std::vec![#(#bindings),*])?; },
        )
    } else {
        let methods = methods
            .iter()
            .map(|method| {
                if !method.signature.generics.params.is_empty() {
                    return Err(SyntaxError::new_spanned(
                        &method.signature,
                        "method-local generics are unsupported",
                    ));
                }
                bound_method(method, runtime)
            })
            .collect::<SyntaxResult<Vec<_>>>()?;
        Ok(
            quote! { #builder.inherent_impl(&[#(#names),*], #receiver, ::std::vec![#(#methods),*])?; },
        )
    }
}

fn free_function(method: Method, runtime: &Path) -> SyntaxResult<TokenStream> {
    let names = generic_names(&method.signature.generics)?;
    let builder = builder_ident();
    if method.signature.receiver().is_some() {
        return Err(SyntaxError::new_spanned(
            &method.signature,
            "free native functions cannot have a receiver",
        ));
    }
    let method = bound_method(&method, runtime)?;
    Ok(quote! { #builder.free_function(&[#(#names),*], #method)?; })
}

fn builder_ident() -> Ident {
    Ident::new("__kagari_native_builder", Span::mixed_site())
}

fn bound_method(method: &Method, runtime: &Path) -> SyntaxResult<TokenStream> {
    method_record(
        &method.signature,
        &method.attrs,
        Some(&method.factory),
        runtime,
    )
}

fn method_record(
    signature: &Signature,
    attrs: &[Attribute],
    factory: Option<&Path>,
    runtime: &Path,
) -> SyntaxResult<TokenStream> {
    if signature.constness.is_some()
        || signature.asyncness.is_some()
        || signature.unsafety.is_some()
        || signature.abi.is_some()
        || signature.variadic.is_some()
    {
        return Err(SyntaxError::new_spanned(
            signature,
            "unsupported native function modifier",
        ));
    }
    generic_names(&signature.generics)?;
    let name = signature.ident.to_string();
    let doc = documentation(attrs)?;
    let params = signature
        .inputs
        .iter()
        .map(|input| {
            let (name, ty) = match input {
                FnArg::Receiver(receiver) => {
                    if receiver.reference.is_some()
                        || receiver.mutability.is_some()
                        || receiver.colon_token.is_some()
                    {
                        return Err(SyntaxError::new_spanned(
                            receiver,
                            "only the script receiver `self` is supported",
                        ));
                    }
                    (
                        "self".into(),
                        named_type(&syn::parse_quote!(Self), runtime)?,
                    )
                }
                FnArg::Typed(parameter) => {
                    let Pat::Ident(name) = parameter.pat.as_ref() else {
                        return Err(SyntaxError::new_spanned(
                            parameter,
                            "native parameters need plain names",
                        ));
                    };
                    if name.by_ref.is_some() || name.mutability.is_some() || name.subpat.is_some() {
                        return Err(SyntaxError::new_spanned(
                            parameter,
                            "native parameters need plain names",
                        ));
                    }
                    (name.ident.to_string(), ty(&parameter.ty, runtime)?)
                }
            };
            Ok(quote! { (#name, #ty) })
        })
        .collect::<SyntaxResult<Vec<_>>>()?;
    let result = return_type(&signature.output, runtime)?;
    let binding = if let Some(factory) = factory {
        let binding = binding(factory, runtime)?;
        quote! { ::std::option::Option::Some(#binding) }
    } else {
        quote! { ::std::option::Option::None }
    };
    Ok(quote! {
        #runtime::native_module::Method {
            name: #name, documentation: #doc, params: ::std::vec![#(#params),*], result: #result, binding: #binding,
        }
    })
}

fn binding(factory: &Path, runtime: &Path) -> SyntaxResult<TokenStream> {
    let path = path_names(factory)?.join("::");
    Ok(quote! { #runtime::native_module::Binding { path: #path, factory: #factory } })
}

fn generic_names(generics: &Generics) -> SyntaxResult<Vec<String>> {
    if generics.where_clause.is_some() {
        return Err(SyntaxError::new_spanned(
            generics,
            "native where clauses are unsupported",
        ));
    }
    generics
        .params
        .iter()
        .map(|parameter| {
            if let GenericParam::Type(parameter) = parameter
                && parameter.bounds.is_empty()
                && parameter.default.is_none()
                && parameter.attrs.is_empty()
            {
                return Ok(parameter.ident.to_string());
            }
            Err(SyntaxError::new_spanned(
                parameter,
                "only unbounded type parameters are supported",
            ))
        })
        .collect()
}

fn documentation(attrs: &[Attribute]) -> SyntaxResult<String> {
    let mut lines = vec![];
    for attribute in attrs {
        if let Meta::NameValue(value) = &attribute.meta
            && value.path.is_ident("doc")
            && let Expr::Lit(value) = &value.value
            && let Lit::Str(value) = &value.lit
        {
            let value = value.value();
            lines.push(value.strip_prefix(' ').unwrap_or(&value).to_owned());
            continue;
        }
        return Err(SyntaxError::new_spanned(
            attribute,
            "only literal native documentation attributes are supported",
        ));
    }
    Ok(lines.join("\n"))
}

fn path_names(path: &Path) -> SyntaxResult<Vec<String>> {
    path.segments
        .iter()
        .map(|segment| {
            if !matches!(segment.arguments, PathArguments::None) {
                return Err(SyntaxError::new_spanned(
                    segment,
                    "generic module/factory paths are unsupported",
                ));
            }
            Ok(segment.ident.to_string())
        })
        .collect()
}

fn return_type(output: &ReturnType, runtime: &Path) -> SyntaxResult<TokenStream> {
    match output {
        ReturnType::Default => ty(&syn::parse_quote!(()), runtime),
        ReturnType::Type(_, result) => ty(result, runtime),
    }
}

fn ty(value: &Type, runtime: &Path) -> SyntaxResult<TokenStream> {
    Ok(match value {
        Type::Path(path) if path.qself.is_none() => return named_type(&path.path, runtime),
        Type::Slice(slice) => {
            let item = ty(&slice.elem, runtime)?;
            quote! { #runtime::native_module::TypeExpression::Array(::std::boxed::Box::new(#item)) }
        }
        Type::Tuple(tuple) => {
            let items = tuple
                .elems
                .iter()
                .map(|item| ty(item, runtime))
                .collect::<SyntaxResult<Vec<_>>>()?;
            quote! { #runtime::native_module::TypeExpression::Tuple(::std::vec![#(#items),*]) }
        }
        Type::BareFn(function)
            if function.unsafety.is_none()
                && function.abi.is_none()
                && function.lifetimes.is_none()
                && function.variadic.is_none() =>
        {
            let params = function
                .inputs
                .iter()
                .map(|input| ty(&input.ty, runtime))
                .collect::<SyntaxResult<Vec<_>>>()?;
            let result = return_type(&function.output, runtime)?;
            quote! { #runtime::native_module::TypeExpression::Function { params: ::std::vec![#(#params),*], result: ::std::boxed::Box::new(#result) } }
        }
        Type::Never(_) => {
            quote! { #runtime::native_module::TypeExpression::Named { path: ::std::vec!["!"], arguments: ::std::vec![], bindings: ::std::vec![] } }
        }
        Type::Paren(inner) => return ty(&inner.elem, runtime),
        _ => {
            return Err(SyntaxError::new_spanned(
                value,
                "unsupported native type expression",
            ));
        }
    })
}

fn named_type(path: &Path, runtime: &Path) -> SyntaxResult<TokenStream> {
    let mut names = vec![];
    let mut arguments = vec![];
    let mut bindings = vec![];
    for (index, segment) in path.segments.iter().enumerate() {
        names.push(segment.ident.to_string());
        if !matches!(segment.arguments, PathArguments::None) && index + 1 != path.segments.len() {
            return Err(SyntaxError::new_spanned(
                segment,
                "only the final native type path segment can have arguments",
            ));
        }
        match &segment.arguments {
            PathArguments::None => {}
            PathArguments::AngleBracketed(args) => {
                for argument in &args.args {
                    match argument {
                        GenericArgument::Type(value) => arguments.push(ty(value, runtime)?),
                        GenericArgument::AssocType(binding) if binding.generics.is_none() => {
                            let name = binding.ident.to_string();
                            let value = ty(&binding.ty, runtime)?;
                            bindings.push(quote! { (#name, #value) });
                        }
                        _ => {
                            return Err(SyntaxError::new_spanned(
                                argument,
                                "unsupported native generic argument",
                            ));
                        }
                    }
                }
            }
            _ => {
                return Err(SyntaxError::new_spanned(
                    segment,
                    "use fn(...) -> ... for native callbacks",
                ));
            }
        }
    }
    Ok(
        quote! { #runtime::native_module::TypeExpression::Named { path: ::std::vec![#(#names),*], arguments: ::std::vec![#(#arguments),*], bindings: ::std::vec![#(#bindings),*] } },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unsupported_declarations_before_expansion() {
        for source in [
            "module game::api; type Array<T> = native_array<i32>;",
            "module game::api; trait Bad { fn f(&self); }",
            "module game::api; trait Bad { fn f<T>(self); }",
            "module game::api; fn f(x: &i32) => entry;",
            "module game::api; fn f<T: Copy>() => entry;",
            "module game::api; trait Bad { fn f(self) { } }",
            "module game::api; #[inline] fn f() => entry;",
        ] {
            let module = syn::parse_str::<Module>(source).unwrap();
            assert!(super::module(module).is_err(), "{source}");
        }
    }

    #[test]
    fn supports_a_renamed_runtime_path_and_nested_types() {
        let input = syn::parse_str::<Module>("runtime = my_runtime; module game::api; fn f<T>(callback: fn([T], (i32, bool)) -> Option<T>) => entry;").unwrap();
        assert!(super::module(input).is_ok());
    }
}
