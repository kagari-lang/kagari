//! Attribute expansion keeps original Rust definitions and emits checked adapters.
use crate::signature;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{
    Attribute, Error as SyntaxError, Fields, File, FnArg, Ident, ImplItem, Item, ItemImpl, ItemMod,
    LitInt, LitStr, Meta, Path, Result as SyntaxResult, ReturnType, Signature, Token, TraitItem,
    Type, TypeParamBound,
    parse::{Parse, ParseStream},
    parse_quote, parse_str, parse2,
};

pub(crate) struct Arguments {
    module: LitStr,
    runtime: Path,
}
impl Parse for Arguments {
    fn parse(input: ParseStream<'_>) -> SyntaxResult<Self> {
        let module = input.parse()?;
        let mut runtime = parse_quote!(::kagari_runtime);
        if !input.is_empty() {
            input.parse::<Token![,]>()?;
            let ident: Ident = input.parse()?;
            if ident != "runtime" {
                return Err(SyntaxError::new_spanned(ident, "expected runtime = path"));
            }
            input.parse::<Token![=]>()?;
            runtime = input.parse()?;
        }
        Ok(Self { module, runtime })
    }
}

struct Entry {
    binding: String,
    delay: bool,
}
fn entry(attr: Option<Attribute>, default: String) -> SyntaxResult<Entry> {
    let mut result = Entry {
        binding: default,
        delay: false,
    };
    if let Some(attr) = attr
        && matches!(attr.meta, Meta::List(_))
    {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("binding") {
                result.binding = meta.value()?.parse::<LitStr>()?.value();
            } else if meta.path.is_ident("steps") {
                let steps: LitInt = meta.value()?.parse()?;
                match steps.base10_parse::<usize>()? {
                    1 => result.delay = false,
                    2 => result.delay = true,
                    _ => {
                        return Err(
                            meta.error("immediate adapters support one or two logical steps")
                        );
                    }
                }
            } else {
                return Err(meta.error("expected binding or steps"));
            }
            Ok(())
        })?;
    }
    Ok(result)
}

struct Expansion<'a> {
    runtime: &'a Path,
    adapters: Vec<TokenStream>,
}
impl Expansion<'_> {
    fn factory(
        &mut self,
        sig: &mut Signature,
        attrs: &mut Vec<Attribute>,
        names: &[Ident],
        receiver: Option<&Type>,
        contract: Option<&Path>,
        default: String,
    ) -> SyntaxResult<TokenStream> {
        let runtime = self.runtime;
        let mut checked = sig.clone();
        checked.generics = Default::default();
        signature::validate(&checked)?;
        let entry = entry(signature::marker(attrs, "native"), default)?;
        let adapter = format_ident!("__kagari_adapter_{}", self.adapters.len());
        let mut declarations = vec![];
        let mut arguments = vec![];
        let mut slot = 0usize;
        let concrete_receiver = receiver.map(|ty| signature::concrete(ty, names, runtime, None));
        for arg in &mut sig.inputs {
            match arg {
                FnArg::Receiver(_) => {
                    let ty = concrete_receiver.as_ref().ok_or_else(|| {
                        SyntaxError::new_spanned(&checked, "receiver outside native impl")
                    })?;
                    let var = format_ident!("__argument_{slot}");
                    declarations.push(quote!(let #var = __call.argument::<#ty>(#slot)?;));
                    arguments.push(quote!(&#var));
                    slot += 1;
                }
                FnArg::Typed(arg) => {
                    if signature::marker(&mut arg.attrs, "context").is_some() {
                        // Rust verifies the injected call against the actual parameter type.
                        arguments.push(quote!(__call));
                    } else {
                        let ty = signature::concrete(
                            &arg.ty,
                            names,
                            runtime,
                            concrete_receiver.as_ref(),
                        );
                        let var = format_ident!("__argument_{slot}");
                        declarations.push(quote!(let #var = __call.argument::<#ty>(#slot)?;));
                        arguments.push(quote!(#var));
                        slot += 1;
                    }
                }
            }
        }
        let result: Type = match &sig.output {
            ReturnType::Default => parse_quote!(()),
            ReturnType::Type(_, ty) => *ty.clone(),
        };
        let result = signature::concrete(&result, names, runtime, concrete_receiver.as_ref());
        let method = &sig.ident;
        let invocation = if let Some(receiver) = &concrete_receiver {
            if let Some(contract) = contract {
                let ty: Type = parse_quote!(#contract);
                let contract = signature::concrete(&ty, names, runtime, None);
                quote!(<#receiver as #contract>::#method(#(#arguments),*))
            } else {
                quote!(<#receiver>::#method(#(#arguments),*))
            }
        } else if names.is_empty() {
            quote!(self::#method(#(#arguments),*))
        } else {
            let concrete: Vec<Type> = names
                .iter()
                .enumerate()
                .map(|(slot, _)| parse_quote!(#runtime::native_value::GenericValue<#slot>))
                .collect();
            quote!(self::#method::<#(#concrete),*>(#(#arguments),*))
        };
        let delay = entry.delay;
        self.adapters.push(quote! {
            fn #adapter() -> #runtime::native::factory::NativeFactory {
                #runtime::native::factory::NativeFactory::typed::<#result>(#delay, |__call| {
                    #(#declarations)*
                    ::std::result::Result::Ok(#invocation)
                })
            }
        });
        let binding = entry.binding;
        Ok(quote!(#runtime::native_module::Binding { name: #binding, factory: #adapter }))
    }

    fn implementation(&mut self, implementation: &mut ItemImpl) -> SyntaxResult<TokenStream> {
        let names = signature::generics(&implementation.generics)?;
        let runtime = self.runtime;
        let receiver = &*implementation.self_ty;
        let receiver_expression = signature::value_type(receiver, &names, runtime, None);
        let contract = implementation.trait_.as_ref().map(|(_, path, _)| path);
        let mut methods = vec![];
        let mut bindings = vec![];
        let Type::Path(receiver_path) = receiver else {
            return Err(SyntaxError::new_spanned(
                receiver,
                "native impl requires a named type",
            ));
        };
        let owner = receiver_path
            .path
            .segments
            .last()
            .unwrap()
            .ident
            .to_string();
        for item in &mut implementation.items {
            let ImplItem::Fn(method) = item else {
                return Err(SyntaxError::new_spanned(
                    item,
                    "native impls export methods only",
                ));
            };
            signature::validate(&method.sig)?;
            let original = method.sig.clone();
            let default = format!(
                "{}_{}_{}",
                owner,
                contract
                    .map(|path| path.segments.last().unwrap().ident.to_string())
                    .unwrap_or_else(|| "inherent".into()),
                method.sig.ident
            );
            let binding = self.factory(
                &mut method.sig,
                &mut method.attrs,
                &names,
                Some(receiver),
                contract,
                default,
            )?;
            if contract.is_some() {
                let name = method.sig.ident.to_string();
                bindings.push(quote!((#name, #binding)));
            } else {
                methods.push(signature::method(
                    &original,
                    &method.attrs,
                    &names,
                    runtime,
                    Some(receiver),
                    quote!(::std::option::Option::Some(#binding)),
                )?);
            }
        }
        let generic_names: Vec<_> = names.iter().map(ToString::to_string).collect();
        Ok(if let Some(contract) = contract {
            let contract = signature::nominal(contract, &names, runtime)?;
            quote!(__builder.trait_impl(&[#(#generic_names),*], #receiver_expression, #contract, ::std::vec![#(#bindings),*])?;)
        } else {
            quote!(__builder.inherent_impl(&[#(#generic_names),*], #receiver_expression, ::std::vec![#(#methods),*])?;)
        })
    }
}

pub(crate) fn expand(args: Arguments, mut module: ItemMod) -> SyntaxResult<TokenStream> {
    let runtime = &args.runtime;
    let path: Vec<_> = args.module.value().split("::").map(str::to_owned).collect();
    if path.len() < 2 || path.iter().any(|part| parse_str::<Ident>(part).is_err()) {
        return Err(SyntaxError::new_spanned(
            args.module,
            "native module needs package::module path",
        ));
    }
    let Some((_, items)) = &mut module.content else {
        return Err(SyntaxError::new_spanned(
            module,
            "native_module requires an inline Rust module",
        ));
    };
    let mut declarations = vec![];
    let mut functions = vec![];
    let mut implementations = vec![];
    let mut value_impls = vec![];
    let mut expansion = Expansion {
        runtime,
        adapters: vec![],
    };
    for item in items.iter_mut() {
        match item {
            Item::Struct(ty)
                if ty
                    .attrs
                    .iter()
                    .any(|attr| attr.path().is_ident("native_type")) =>
            {
                signature::marker(&mut ty.attrs, "native_type");
                let names = signature::generics(&ty.generics)?;
                let Fields::Unnamed(fields) = &ty.fields else {
                    return Err(SyntaxError::new_spanned(
                        ty,
                        "native types wrap one NativeArray field",
                    ));
                };
                if fields.unnamed.len() != 1 {
                    return Err(SyntaxError::new_spanned(
                        fields,
                        "native types wrap one NativeArray field",
                    ));
                }
                let field = &fields.unnamed[0].ty;
                let Type::Path(field_path) = field else {
                    return Err(SyntaxError::new_spanned(field, "expected NativeArray<T>"));
                };
                if field_path
                    .path
                    .segments
                    .last()
                    .is_none_or(|part| part.ident != "NativeArray")
                {
                    return Err(SyntaxError::new_spanned(
                        field,
                        "this native type adapter supports NativeArray storage",
                    ));
                }
                let ident = &ty.ident;
                let name = ident.to_string();
                let doc = signature::documentation(&ty.attrs);
                let generic_names: Vec<_> = names.iter().map(ToString::to_string).collect();
                declarations
                    .push(quote!(__builder.array_type(#name, &[#(#generic_names),*], #doc)?;));
                let (impl_generics, type_generics, where_clause) = ty.generics.split_for_impl();
                value_impls.push(quote! {
                    impl #impl_generics #runtime::native_value::NativeValue for #ident #type_generics #where_clause {
                        fn type_expression(__names: &[&'static str]) -> #runtime::native_module::types::TypeExpression {
                            <#field as #runtime::native_value::NativeValue>::type_expression(__names)
                        }
                        fn read(__call: &#runtime::native_value::NativeCall, __value: #runtime::value::Value, __expected: &::kagari_abi::types::AbiType) -> #runtime::native_value::NativeResult<Self> {
                            ::std::result::Result::Ok(Self(<#field as #runtime::native_value::NativeValue>::read(__call, __value, __expected)?))
                        }
                        fn write(self, __call: &#runtime::native_value::NativeCall, __expected: &::kagari_abi::types::AbiType) -> #runtime::native_value::NativeResult<#runtime::value::Value> {
                            <#field as #runtime::native_value::NativeValue>::write(self.0, __call, __expected)
                        }
                    }
                });
            }
            Item::Trait(ty)
                if ty
                    .attrs
                    .iter()
                    .any(|attr| attr.path().is_ident("native_trait")) =>
            {
                signature::marker(&mut ty.attrs, "native_trait");
                let names = signature::generics(&ty.generics)?;
                let name = ty.ident.to_string();
                let doc = signature::documentation(&ty.attrs);
                let generic_names: Vec<_> = names.iter().map(ToString::to_string).collect();
                let parents = ty
                    .supertraits
                    .iter()
                    .map(|parent| {
                        let TypeParamBound::Trait(bound) = parent else {
                            return Err(SyntaxError::new_spanned(
                                parent,
                                "native parents must be declared traits",
                            ));
                        };
                        signature::nominal(&bound.path, &names, runtime)
                    })
                    .collect::<SyntaxResult<Vec<_>>>()?;
                let methods = ty.items.iter().map(|item| {
                    let TraitItem::Fn(method) = item else { return Err(SyntaxError::new_spanned(item, "native traits export required methods only")); };
                    if method.default.is_some() { return Err(SyntaxError::new_spanned(method, "native trait defaults require a separate executable implementation")); }
                    signature::validate(&method.sig)?;
                    signature::method(&method.sig, &method.attrs, &names, runtime, None, quote!(::std::option::Option::None))
                }).collect::<SyntaxResult<Vec<_>>>()?;
                declarations.push(quote!(__builder.required_trait(#name, &[#(#generic_names),*], #doc, ::std::vec![#(#parents),*], ::std::vec![#(#methods),*])?;));
            }
            Item::Impl(implementation)
                if implementation
                    .attrs
                    .iter()
                    .any(|attr| attr.path().is_ident("native_impl")) =>
            {
                signature::marker(&mut implementation.attrs, "native_impl");
                implementations.push(expansion.implementation(implementation)?);
            }
            Item::Fn(function)
                if function
                    .attrs
                    .iter()
                    .any(|attr| attr.path().is_ident("native")) =>
            {
                let names = signature::generics(&function.sig.generics)?;
                let original = function.sig.clone();
                let default = function.sig.ident.to_string();
                let binding = expansion.factory(
                    &mut function.sig,
                    &mut function.attrs,
                    &names,
                    None,
                    None,
                    default,
                )?;
                let method = signature::method(
                    &original,
                    &function.attrs,
                    &names,
                    runtime,
                    None,
                    quote!(::std::option::Option::Some(#binding)),
                )?;
                let generic_names: Vec<_> = names.iter().map(ToString::to_string).collect();
                functions.push(quote!(__builder.free_function(&[#(#generic_names),*], #method)?;));
            }
            _ => {}
        }
    }
    let adapters = expansion.adapters;
    let generated: File = parse2(quote! {
        #(#value_impls)*
        #(#adapters)*
        /// Build the validated executable API and its generated tooling declarations.
        pub fn native_api() -> ::std::result::Result<#runtime::native::api::NativeApi, #runtime::error::RuntimeError> {
            let mut __builder = #runtime::native_module::NativeModuleBuilder::new(&[#(#path),*])?;
            #(#declarations)* #(#implementations)* #(#functions)*
            __builder.finish()
        }
    })?;
    items.extend(generated.items);
    Ok(quote!(#module))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unsupported_rust_contracts_report_errors() {
        for module in [
            quote!(
                mod native {
                    #[native]
                    async fn bad() {}
                }
            ),
            quote!(
                mod native {
                    #[native]
                    fn bad<'a>(arg: &'a str) {}
                }
            ),
            quote!(
                mod native {
                    #[native_type]
                    struct Bad<T: NativeValue> {
                        value: T,
                    }
                }
            ),
            quote!(
                mod native {
                    #[native_trait]
                    trait Bad<T: NativeValue> {
                        fn value(&self) -> T {
                            panic!()
                        }
                    }
                }
            ),
        ] {
            assert!(
                expand(
                    Arguments {
                        module: parse_quote!("game::native"),
                        runtime: parse_quote!(::kagari_runtime)
                    },
                    syn::parse2(module).unwrap()
                )
                .is_err()
            );
        }
    }
}
