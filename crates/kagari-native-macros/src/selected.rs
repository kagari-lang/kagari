//! Selected dependencies are injected Rust arguments and checked script bounds.
use crate::signature;
use proc_macro2::TokenStream;
use quote::quote;
use syn::{
    Attribute, Error as SyntaxError, FnArg, Ident, Path, PathArguments, Result as SyntaxResult,
    Signature, Token, Type,
    parse::{Parse, ParseStream},
};

pub(crate) struct Selected {
    pub receiver: Type,
    pub contract: Path,
    pub member: Ident,
}
impl Parse for Selected {
    fn parse(input: ParseStream<'_>) -> SyntaxResult<Self> {
        let receiver = input.parse()?;
        input.parse::<Token![:]>()?;
        let mut contract: Path = input.parse()?;
        let member = contract
            .segments
            .pop()
            .ok_or_else(|| input.error("expected Trait::method"))?
            .into_value();
        if contract.segments.is_empty() || !member.arguments.is_empty() {
            return Err(input.error("selected dependencies use Receiver: Trait::method"));
        }
        contract.segments.pop_punct();
        Ok(Self {
            receiver,
            contract,
            member: member.ident,
        })
    }
}

pub(crate) fn marker(attrs: &[Attribute]) -> Option<&Attribute> {
    attrs.iter().find(|attr| attr.path().is_ident("selected"))
}

pub(crate) fn descriptors(
    signature: &Signature,
    names: &[Ident],
    runtime: &Path,
    receiver: Option<&Type>,
) -> SyntaxResult<Vec<TokenStream>> {
    let mut descriptors = vec![];
    let generic_names: Vec<_> = names.iter().map(ToString::to_string).collect();
    for argument in &signature.inputs {
        let FnArg::Typed(argument) = argument else {
            continue;
        };
        let Some(attr) = marker(&argument.attrs) else {
            continue;
        };
        if argument
            .attrs
            .iter()
            .filter(|attr| attr.path().is_ident("selected"))
            .count()
            != 1
            || argument
                .attrs
                .iter()
                .any(|attr| attr.path().is_ident("context"))
        {
            return Err(SyntaxError::new_spanned(
                argument,
                "selected parameters require one selection and no context marker",
            ));
        }
        let selected: Selected = attr.parse_args()?;
        let selected_receiver = receiver_expression(&selected.receiver, names, runtime, receiver)?;
        let contract = signature::nominal(&selected.contract, names, runtime)?;
        let member = selected.member.to_string();
        let ty = signature::concrete(&argument.ty, names, runtime, receiver);
        descriptors.push(quote!(#runtime::native_module::Selected {
            receiver: #selected_receiver, interface: #contract, member: #member,
            signature: <#ty>::signature_expression(&[#(#generic_names),*]),
        }));
    }
    Ok(descriptors)
}

fn receiver_expression(
    ty: &Type,
    names: &[Ident],
    runtime: &Path,
    receiver: Option<&Type>,
) -> SyntaxResult<TokenStream> {
    let Type::Path(path) = ty else {
        return Ok(signature::value_type(ty, names, runtime, receiver));
    };
    let Some(qualified) = &path.qself else {
        return Ok(signature::value_type(ty, names, runtime, receiver));
    };
    if qualified.position == 0 || path.path.segments.len() != qualified.position + 1 {
        return Err(SyntaxError::new_spanned(
            ty,
            "selected projections use <Receiver as Trait>::Member",
        ));
    }
    let mut interface = path.path.clone();
    let member = interface
        .segments
        .pop()
        .expect("checked projection member")
        .into_value();
    if !matches!(member.arguments, PathArguments::None) {
        return Err(SyntaxError::new_spanned(
            member,
            "selected associated families are not supported",
        ));
    }
    interface.segments.pop_punct();
    let receiver = receiver_expression(&qualified.ty, names, runtime, receiver)?;
    let interface = signature::nominal(&interface, names, runtime)?;
    let member = member.ident.to_string();
    Ok(
        quote!(#runtime::native_module::types::TypeExpression::Projection {
            receiver: ::std::boxed::Box::new(#receiver),
            interface: ::std::boxed::Box::new(#interface), member: #member,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    #[test]
    fn selected_projection_syntax_requires_a_qualified_ordinary_member() {
        let runtime = parse_quote!(kagari_runtime);
        let names = [parse_quote!(T), parse_quote!(U)];
        for ty in [
            parse_quote!(<T>::Output),
            parse_quote!(<T as Reader>::Output::Nested),
            parse_quote!(<T as Reader>::Output<U>),
        ] {
            assert!(receiver_expression(&ty, &names, &runtime, None).is_err());
        }
    }
}
