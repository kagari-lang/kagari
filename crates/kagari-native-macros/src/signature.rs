//! Rust signatures and type substitution for a single compiled generic adapter.
use crate::selected;
use proc_macro2::TokenStream;
use quote::quote;
use syn::{
    Attribute, Error as SyntaxError, Expr, FnArg, GenericArgument, GenericParam, Generics, Ident,
    Lit, Meta, Pat, Path, PathArguments, Result as SyntaxResult, ReturnType, Signature, Type,
    TypeParamBound, parse_quote,
    visit_mut::{self, VisitMut},
};

pub(crate) fn generics(input: &Generics) -> SyntaxResult<Vec<Ident>> {
    if input.where_clause.is_some() {
        return Err(SyntaxError::new_spanned(
            input,
            "native generics use inline NativeValue bounds",
        ));
    }
    input.params.iter().map(|parameter| {
        let GenericParam::Type(parameter) = parameter else {
            return Err(SyntaxError::new_spanned(parameter, "native lifetimes and const generics are unsupported"));
        };
        if parameter.default.is_some() || parameter.bounds.len() != 1 || !matches!(parameter.bounds.first(), Some(TypeParamBound::Trait(bound)) if bound.path.is_ident("NativeValue")) {
            return Err(SyntaxError::new_spanned(parameter, "native generic parameters require exactly T: NativeValue"));
        }
        Ok(parameter.ident.clone())
    }).collect()
}

pub(crate) fn documentation(attrs: &[Attribute]) -> String {
    attrs
        .iter()
        .filter_map(|attr| {
            let Meta::NameValue(value) = &attr.meta else {
                return None;
            };
            let Expr::Lit(value) = &value.value else {
                return None;
            };
            let Lit::Str(value) = &value.lit else {
                return None;
            };
            attr.path()
                .is_ident("doc")
                .then(|| value.value().trim().to_owned())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn marker(attrs: &mut Vec<Attribute>, name: &str) -> Option<Attribute> {
    let index = attrs.iter().position(|attr| attr.path().is_ident(name))?;
    Some(attrs.remove(index))
}

struct Substitute<'a> {
    names: &'a [Ident],
    runtime: &'a Path,
    receiver: Option<&'a Type>,
    contract: Option<&'a Type>,
}
impl VisitMut for Substitute<'_> {
    fn visit_type_mut(&mut self, ty: &mut Type) {
        if let Type::Path(path) = ty
            && path.qself.is_none()
            && path.path.segments.len() == 2
            && path.path.segments[0].ident == "Self"
            && let (Some(receiver), Some(contract)) = (self.receiver, self.contract)
        {
            let member = &path.path.segments[1];
            *ty = parse_quote!(<#receiver as #contract>::#member);
            visit_mut::visit_type_mut(self, ty);
            return;
        }
        if let Type::Path(path) = ty
            && path.qself.is_none()
            && path.path.segments.len() == 1
        {
            let ident = &path.path.segments[0].ident;
            if ident == "Self"
                && let Some(receiver) = self.receiver
            {
                *ty = receiver.clone();
                self.visit_type_mut(ty);
                return;
            }
            if let Some(slot) = self.names.iter().position(|name| name == ident) {
                let runtime = self.runtime;
                *ty = parse_quote!(#runtime::native_value::GenericValue<#slot>);
                return;
            }
        }
        visit_mut::visit_type_mut(self, ty);
    }
}

pub(crate) fn concrete(
    ty: &Type,
    names: &[Ident],
    runtime: &Path,
    receiver: Option<&Type>,
) -> Type {
    concrete_method(ty, names, runtime, receiver, None)
}

pub(crate) fn concrete_method(
    ty: &Type,
    names: &[Ident],
    runtime: &Path,
    receiver: Option<&Type>,
    contract: Option<&Type>,
) -> Type {
    let mut ty = ty.clone();
    Substitute {
        names,
        runtime,
        receiver,
        contract,
    }
    .visit_type_mut(&mut ty);
    ty
}

/// Trait declarations need symbolic slots; implementation adapters use Rust's
/// actual associated types and qualified trait conformance instead.
pub(crate) fn trait_signature(
    signature: &Signature,
    owner: &Ident,
    names: &[Ident],
    associated: &[Ident],
    runtime: &Path,
) -> Signature {
    struct Slots<'a> {
        owner: &'a Ident,
        names: &'a [Ident],
        associated: &'a [Ident],
        runtime: &'a Path,
    }
    impl VisitMut for Slots<'_> {
        fn visit_type_mut(&mut self, ty: &mut Type) {
            if let Type::Path(path) = ty
                && let Some(qualified) = &path.qself
                && matches!(&*qualified.ty, Type::Path(receiver) if receiver.qself.is_none() && receiver.path.is_ident("Self"))
                && path.path.segments.len() == 2
                && path.path.segments[0].ident == *self.owner
                && declaration_arguments(&path.path.segments[0].arguments, self.names)
                && let Some(slot) = self.associated.iter().position(|name| {
                    *name == path.path.segments.last().expect("associated path").ident
                })
            {
                let runtime = self.runtime;
                *ty = parse_quote!(#runtime::native_value::declaration::AssociatedValue<#slot>);
                return;
            }
            if let Type::Path(path) = ty
                && path.qself.is_none()
                && path.path.segments[0].ident == "Self"
            {
                let runtime = self.runtime;
                if path.path.segments.len() == 1 {
                    *ty = parse_quote!(#runtime::native_value::declaration::SelfValue);
                    return;
                }
                if path.path.segments.len() == 2
                    && let Some(slot) = self
                        .associated
                        .iter()
                        .position(|name| *name == path.path.segments[1].ident)
                {
                    *ty = parse_quote!(#runtime::native_value::declaration::AssociatedValue<#slot>);
                    return;
                }
            }
            visit_mut::visit_type_mut(self, ty);
        }
    }
    let mut signature = signature.clone();
    Slots {
        owner,
        names,
        associated,
        runtime,
    }
    .visit_signature_mut(&mut signature);
    signature
}

fn declaration_arguments(arguments: &PathArguments, names: &[Ident]) -> bool {
    match arguments {
        PathArguments::None => names.is_empty(),
        PathArguments::AngleBracketed(arguments) => {
            arguments.args.len() == names.len()
                && arguments.args.iter().zip(names).all(|(argument, name)| {
                    matches!(argument, GenericArgument::Type(Type::Path(path)) if path.qself.is_none() && path.path.is_ident(name))
                })
        }
        PathArguments::Parenthesized(_) => false,
    }
}

pub(crate) fn value_type(
    ty: &Type,
    names: &[Ident],
    runtime: &Path,
    receiver: Option<&Type>,
) -> TokenStream {
    let ty = concrete(ty, names, runtime, receiver);
    let names: Vec<_> = names.iter().map(ToString::to_string).collect();
    quote!(<#ty as #runtime::native_value::NativeValue>::type_expression(&[#(#names),*]))
}

pub(crate) fn nominal(path: &Path, names: &[Ident], runtime: &Path) -> SyntaxResult<TokenStream> {
    let mut segments: Vec<_> = path.segments.iter().map(|s| s.ident.to_string()).collect();
    let last = path
        .segments
        .last()
        .ok_or_else(|| SyntaxError::new_spanned(path, "native trait path is empty"))?;
    if last.ident == "NativeIndex" {
        segments = vec!["std".into(), "ops".into(), "Index".into()];
    }
    let mut arguments = vec![];
    let mut bindings = vec![];
    if let PathArguments::AngleBracketed(args) = &last.arguments {
        for arg in &args.args {
            match arg {
                GenericArgument::Type(ty) => arguments.push(value_type(ty, names, runtime, None)),
                GenericArgument::AssocType(binding) if binding.generics.is_none() => {
                    let name = binding.ident.to_string();
                    let ty = value_type(&binding.ty, names, runtime, None);
                    bindings.push(quote!((#name, #ty)));
                }
                _ => {
                    return Err(SyntaxError::new_spanned(
                        arg,
                        "unsupported native trait argument",
                    ));
                }
            }
        }
    } else if !matches!(last.arguments, PathArguments::None) {
        return Err(SyntaxError::new_spanned(
            path,
            "unsupported native trait arguments",
        ));
    }
    Ok(
        quote!(#runtime::native_module::types::TypeExpression::Named {
            path: ::std::vec![#(#segments),*], arguments: ::std::vec![#(#arguments),*], bindings: ::std::vec![#(#bindings),*],
        }),
    )
}

pub(crate) fn validate(sig: &Signature) -> SyntaxResult<()> {
    if sig.asyncness.is_some()
        || sig.unsafety.is_some()
        || sig.abi.is_some()
        || sig.variadic.is_some()
        || !sig.generics.params.is_empty()
        || sig.generics.where_clause.is_some()
    {
        return Err(SyntaxError::new_spanned(
            sig,
            "native functions must be synchronous safe Rust functions; generics belong to the module function or impl",
        ));
    }
    Ok(())
}

pub(crate) fn method(
    sig: &Signature,
    attrs: &[Attribute],
    names: &[Ident],
    runtime: &Path,
    receiver: Option<&Type>,
    binding: TokenStream,
) -> SyntaxResult<TokenStream> {
    let name = sig.ident.to_string();
    let doc = documentation(attrs);
    let mut params = vec![];
    for arg in &sig.inputs {
        match arg {
            FnArg::Receiver(arg) => {
                if arg.reference.is_none() || arg.mutability.is_some() {
                    return Err(SyntaxError::new_spanned(
                        arg,
                        "native receivers use &self with checked handles",
                    ));
                }
                let ty = if let Some(receiver) = receiver {
                    value_type(receiver, names, runtime, None)
                } else {
                    quote!(#runtime::native_module::types::TypeExpression::Named { path: ::std::vec!["Self"], arguments: ::std::vec![], bindings: ::std::vec![] })
                };
                params.push(quote!(("self", #ty)));
            }
            FnArg::Typed(arg) => {
                if arg.attrs.iter().any(|attr| attr.path().is_ident("context"))
                    || selected::marker(&arg.attrs).is_some()
                {
                    continue;
                }
                let Pat::Ident(pattern) = &*arg.pat else {
                    return Err(SyntaxError::new_spanned(
                        arg,
                        "native arguments require named identifiers",
                    ));
                };
                let name = pattern.ident.to_string();
                let ty = value_type(&arg.ty, names, runtime, receiver);
                params.push(quote!((#name, #ty)));
            }
        }
    }
    let result: Type = match &sig.output {
        ReturnType::Default => parse_quote!(()),
        ReturnType::Type(_, ty) => *ty.clone(),
    };
    let result = concrete(&result, names, runtime, receiver);
    let selected = selected::descriptors(sig, names, runtime, receiver)?;
    let names: Vec<_> = names.iter().map(ToString::to_string).collect();
    Ok(quote!(#runtime::native_module::Method {
        name: #name, documentation: #doc, params: ::std::vec![#(#params),*],
        result: <#result as #runtime::native_value::NativeReturn>::type_expression(&[#(#names),*]), binding: #binding,
        selected: ::std::vec![#(#selected),*],
    }))
}
