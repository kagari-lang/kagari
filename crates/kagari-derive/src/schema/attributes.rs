//! Names follow the Serde data model; unsupported representation changes reject.
use proc_macro2::{Span, TokenStream};
use quote::ToTokens;
use syn::{Attribute, Error, Ident, LitStr, Result, parse_str, token};

#[derive(Clone, Copy)]
pub(super) enum Location {
    Container,
    Field,
    TupleField,
    Variant,
}

#[derive(Default)]
pub(super) struct Attributes {
    pub(super) rename: Option<String>,
    pub(super) rename_all: Option<String>,
    pub(super) kagari_name: Option<String>,
    pub(super) skip: bool,
}

impl Attributes {
    pub(super) fn parse(attributes: &[Attribute], location: Location) -> Result<Self> {
        let mut options = Self::default();
        for attribute in attributes {
            if attribute.path().is_ident("serde") {
                attribute.parse_nested_meta(|meta| {
                    if meta.path.is_ident("rename") && !matches!(location, Location::TupleField) {
                        options.rename = Some(meta.value()?.parse::<LitStr>().map_err(|_| meta.error("only symmetric string rename is supported"))?.value());
                    } else if meta.path.is_ident("rename_all") && matches!(location, Location::Container) {
                        let rule = meta.value()?.parse::<LitStr>().map_err(|_| meta.error("only symmetric string rename_all is supported"))?;
                        if !matches!(rule.value().as_str(), "lowercase" | "UPPERCASE" | "PascalCase" | "camelCase" | "snake_case" | "SCREAMING_SNAKE_CASE" | "kebab-case" | "SCREAMING-KEBAB-CASE") {
                            return Err(meta.error("unsupported Serde rename_all rule"));
                        }
                        options.rename_all = Some(rule.value());
                    } else if meta.path.is_ident("skip") && !matches!(location, Location::Container) {
                        options.skip = true;
                    } else if meta.path.is_ident("default") && !matches!(location, Location::Variant) {
                        if meta.input.peek(token::Eq) { let _: LitStr = meta.value()?.parse()?; }
                    } else {
                        return Err(meta.error("unsupported Serde attribute: KagariSchema accepts symmetric rename/rename_all and unconditional skip/default; changed or asymmetric representations require an explicit schema/converter"));
                    }
                    Ok(())
                })?;
            } else if attribute.path().is_ident("kagari") {
                if !matches!(location, Location::Container) {
                    return Err(Error::new_spanned(
                        attribute,
                        "KagariSchema only accepts a container name override",
                    ));
                }
                attribute.parse_nested_meta(|meta| {
                    if !meta.path.is_ident("name") {
                        return Err(meta.error("unsupported KagariSchema attribute"));
                    }
                    options.kagari_name = Some(meta.value()?.parse::<LitStr>()?.value());
                    Ok(())
                })?;
            } else if attribute.path().is_ident("repr") {
                attribute.parse_nested_meta(|meta| {
                    if meta.path.is_ident("packed") {
                        return Err(
                            meta.error("KagariSchema does not support packed representations")
                        );
                    }
                    if meta.input.peek(token::Paren) {
                        let content;
                        syn::parenthesized!(content in meta.input);
                        let _: TokenStream = content.parse()?;
                    }
                    Ok(())
                })?;
            }
        }
        Ok(options)
    }

    pub(super) fn name(&self, original: &str, rule: Option<&str>, variant: bool) -> Result<String> {
        if let Some(name) = &self.rename {
            return Ok(name.clone());
        }
        let Some(rule) = rule else {
            return Ok(original.to_owned());
        };
        let snake_variant = || {
            let mut value = String::new();
            for (index, ch) in original.char_indices() {
                if index > 0 && ch.is_uppercase() {
                    value.push('_');
                }
                value.push(ch.to_ascii_lowercase());
            }
            value
        };
        let pascal_field = || {
            let mut value = String::new();
            let mut uppercase = true;
            for ch in original.chars() {
                if ch == '_' {
                    uppercase = true;
                    continue;
                }
                value.push(if uppercase {
                    ch.to_ascii_uppercase()
                } else {
                    ch
                });
                uppercase = false;
            }
            value
        };
        let lowercase_first = |value: String| {
            let mut chars = value.chars();
            match chars.next() {
                Some(first) => first.to_ascii_lowercase().to_string() + chars.as_str(),
                None => value,
            }
        };
        let result = match rule {
            "lowercase" if variant => original.to_ascii_lowercase(),
            "lowercase" => original.to_owned(),
            "UPPERCASE" => original.to_ascii_uppercase(),
            "PascalCase" if variant => original.to_owned(),
            "PascalCase" => pascal_field(),
            "camelCase" if variant => lowercase_first(original.to_owned()),
            "camelCase" => lowercase_first(pascal_field()),
            "snake_case" if variant => snake_variant(),
            "snake_case" => original.to_owned(),
            "SCREAMING_SNAKE_CASE" if variant => snake_variant().to_ascii_uppercase(),
            "SCREAMING_SNAKE_CASE" => original.to_ascii_uppercase(),
            "kebab-case" if variant => snake_variant().replace('_', "-"),
            "kebab-case" => original.replace('_', "-"),
            "SCREAMING-KEBAB-CASE" if variant => {
                snake_variant().to_ascii_uppercase().replace('_', "-")
            }
            "SCREAMING-KEBAB-CASE" => original.to_ascii_uppercase().replace('_', "-"),
            _ => {
                return Err(Error::new(Span::call_site(), "unsupported rename rule"));
            }
        };
        Ok(result)
    }
}

pub(super) fn short_name(name: &str, owner: impl ToTokens) -> Result<()> {
    if name.contains('#') || parse_str::<Ident>(name).is_err() {
        return Err(Error::new_spanned(
            owner,
            "Serde schema names must be short script identifiers",
        ));
    }
    Ok(())
}
