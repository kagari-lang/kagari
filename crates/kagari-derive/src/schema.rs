//! Static Serde shape discovery honors only the reviewed finite attribute subset.
mod attributes;
use attributes::{Attributes, Location, short_name};
use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Error, Fields, Result, ext::IdentExt};

pub(super) fn expand(input: &DeriveInput) -> Result<TokenStream> {
    if !input.generics.params.is_empty() || input.generics.where_clause.is_some() {
        return Err(Error::new_spanned(
            &input.generics,
            "KagariSchema requires a finite nongeneric owned Rust type",
        ));
    }
    let attributes = Attributes::parse(&input.attrs, Location::Container)?;
    let ident = &input.ident;
    let name = attributes
        .kagari_name
        .as_ref()
        .or(attributes.rename.as_ref())
        .cloned()
        .unwrap_or_else(|| ident.unraw().to_string());
    short_name(&name, ident)?;
    let body = match &input.data {
        Data::Struct(data) => {
            let Fields::Named(fields) = &data.fields else {
                return Err(Error::new_spanned(
                    ident,
                    "KagariSchema structs require named fields",
                ));
            };
            let mut schemas = vec![];
            for field in &fields.named {
                let options = Attributes::parse(&field.attrs, Location::Field)?;
                if options.skip {
                    continue;
                }
                let member = field.ident.as_ref().expect("named field");
                let name = options.name(
                    &member.unraw().to_string(),
                    attributes.rename_all.as_deref(),
                    false,
                )?;
                short_name(&name, member)?;
                let ty = &field.ty;
                schemas.push(quote! { ::kagari_runtime::native::schema::ValueField::new(#name, registration.dependency::<::kagari_runtime::native::serde::Serde<#ty>>()?) });
            }
            let registration = if schemas.is_empty() {
                quote! { _registration }
            } else {
                quote! { registration }
            };
            quote! { registration.structure::<::kagari_runtime::native::serde::Serde<Self>>(#name, |#registration| Ok(vec![#(#schemas),*])) }
        }
        Data::Enum(data) => {
            let mut schemas = vec![];
            let mut payloads = false;
            for variant in &data.variants {
                let options = Attributes::parse(&variant.attrs, Location::Variant)?;
                if options.skip {
                    continue;
                }
                let name = options.name(
                    &variant.ident.unraw().to_string(),
                    attributes.rename_all.as_deref(),
                    true,
                )?;
                short_name(&name, &variant.ident)?;
                let fields = match &variant.fields {
                    Fields::Unit => vec![],
                    Fields::Unnamed(fields) if fields.unnamed.len() <= 12 => {
                        fields.unnamed.iter().collect()
                    }
                    _ => {
                        return Err(Error::new_spanned(
                            &variant.ident,
                            "KagariSchema variants require unit or tuple payloads of at most twelve fields",
                        ));
                    }
                };
                let mut types = vec![];
                for field in fields {
                    let options = Attributes::parse(&field.attrs, Location::TupleField)?;
                    if !options.skip {
                        types.push(&field.ty);
                    }
                }
                payloads |= !types.is_empty();
                schemas.push(quote! { ::kagari_runtime::native::schema::ValueVariant::new(#name, [#(registration.dependency::<::kagari_runtime::native::serde::Serde<#types>>()?),*]) });
            }
            if schemas.is_empty() {
                return Err(Error::new_spanned(
                    ident,
                    "KagariSchema requires at least one non-skipped variant",
                ));
            }
            let registration = if payloads {
                quote! { registration }
            } else {
                quote! { _registration }
            };
            quote! { registration.enumeration::<::kagari_runtime::native::serde::Serde<Self>>(#name, |#registration| Ok(vec![#(#schemas),*])) }
        }
        Data::Union(_) => {
            return Err(Error::new_spanned(
                ident,
                "KagariSchema does not support unions",
            ));
        }
    };
    Ok(quote! {
        impl ::kagari_runtime::native::serde::KagariSchema for #ident {
            fn schema_type(catalog: &::kagari_runtime::native::catalog::DeclarationCatalog) -> ::kagari_runtime::native::binding::NativeResult<::kagari_runtime::native::types::Type> {
                catalog.rust_type::<::kagari_runtime::native::serde::Serde<Self>>()
            }

            fn register_schema(registration: &mut ::kagari_runtime::native::schema::TypeRegistration<'_>) -> ::kagari_runtime::native::binding::NativeResult<::kagari_runtime::native::types::Type> {
                #body
            }
        }
    })
}
