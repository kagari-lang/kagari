//! Native defaults derive script members from executable Rust function templates.
use crate::{selected::Selected, signature};
use proc_macro2::TokenStream;
use quote::quote;
use syn::{
    Error as SyntaxError, Ident, Path, Result as SyntaxResult, Token,
    ext::IdentExt,
    parse::{Parse, ParseStream},
};

pub(crate) struct Default {
    selection: Selected,
    final_method: bool,
}
impl Parse for Default {
    fn parse(input: ParseStream<'_>) -> SyntaxResult<Self> {
        let selection = input.parse()?;
        let final_method = if input.is_empty() {
            false
        } else {
            input.parse::<Token![,]>()?;
            let policy = Ident::parse_any(input)?;
            if policy != "final" {
                return Err(SyntaxError::new_spanned(policy, "expected final"));
            }
            true
        };
        Ok(Self {
            selection,
            final_method,
        })
    }
}
impl Default {
    pub(crate) fn descriptor(&self, names: &[Ident], runtime: &Path) -> SyntaxResult<TokenStream> {
        let receiver = signature::value_type(&self.selection.receiver, names, runtime, None);
        let interface = signature::nominal(&self.selection.contract, names, runtime)?;
        let member = self.selection.member.to_string();
        let final_method = self.final_method;
        Ok(quote!(#runtime::native_module::DefaultMember {
            receiver: #receiver, interface: #interface, member: #member, final_method: #final_method,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;
    use syn::parse2;

    #[test]
    fn default_policy_and_mapping_syntax_are_explicit() {
        for syntax in [
            quote!(T: Source<U, Output = R>::echo),
            quote!(T: Source<U, Output = R>::echo, final),
        ] {
            assert!(parse2::<Default>(syntax).is_ok());
        }
        for syntax in [
            quote!(T: Source::echo, Final),
            quote!(T: Source::echo, final, final),
            quote!(T: Source),
            quote!(T: Source::echo<i32>),
        ] {
            assert!(parse2::<Default>(syntax).is_err());
        }
    }
}
