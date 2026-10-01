//! Explicit script identities keep Rust parent imports and aliases independent.
use crate::signature;
use proc_macro2::TokenStream;
use syn::{
    Attribute, Error as SyntaxError, Ident, ItemTrait, LitStr, Meta, Path, PathArguments,
    Result as SyntaxResult, Token, TypeParamBound, parenthesized, punctuated::Punctuated,
};

pub(crate) fn descriptors(
    marker: Attribute,
    declaration: &ItemTrait,
    names: &[Ident],
    runtime: &Path,
) -> SyntaxResult<Vec<TokenStream>> {
    let mut mappings = None;
    if matches!(marker.meta, Meta::List(_)) {
        marker.parse_nested_meta(|meta| {
            if !meta.path.is_ident("parents") || mappings.is_some() {
                return Err(meta.error("expected one parents(\"pkg::mod::Trait\", ...) mapping"));
            }
            let content;
            parenthesized!(content in meta.input);
            let paths = Punctuated::<LitStr, Token![,]>::parse_terminated(&content)?;
            mappings = Some(
                paths
                    .iter()
                    .map(|text| {
                        let path: Path = text.parse()?;
                        if path.leading_colon.is_some()
                            || path.segments.len() < 3
                            || path
                                .segments
                                .iter()
                                .any(|part| !matches!(part.arguments, PathArguments::None))
                        {
                            return Err(SyntaxError::new_spanned(
                                text,
                                "parent mapping names a script package, module and trait; arguments come from the Rust parent",
                            ));
                        }
                        Ok(path)
                    })
                    .collect::<SyntaxResult<Vec<_>>>()?,
            );
            Ok(())
        })?;
    }
    if let Some(paths) = &mappings
        && paths.len() != declaration.supertraits.len()
    {
        return Err(SyntaxError::new_spanned(
            marker,
            "parent mappings must match the Rust supertraits in declaration order",
        ));
    }
    declaration
        .supertraits
        .iter()
        .enumerate()
        .map(|(index, parent)| {
            let TypeParamBound::Trait(bound) = parent else {
                return Err(SyntaxError::new_spanned(
                    parent,
                    "native parents must be declared traits",
                ));
            };
            let mut path = mappings
                .as_ref()
                .map(|paths| paths[index].clone())
                .unwrap_or_else(|| bound.path.clone());
            path.segments.last_mut().expect("parent path").arguments = bound
                .path
                .segments
                .last()
                .expect("Rust parent path")
                .arguments
                .clone();
            signature::nominal(&path, names, runtime)
        })
        .collect()
}
