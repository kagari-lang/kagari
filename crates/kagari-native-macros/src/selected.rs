//! Selected dependencies are injected Rust arguments and checked script bounds.
use crate::signature;
use proc_macro2::TokenStream;
use quote::quote;
use syn::{
    Attribute, Error as SyntaxError, FnArg, Ident, Path, Result as SyntaxResult, Signature, Token,
    Type,
    parse::{Parse, ParseStream},
};

struct Selected {
    receiver: Type,
    contract: Path,
    member: Ident,
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
        let selected_receiver = signature::value_type(&selected.receiver, names, runtime, receiver);
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
