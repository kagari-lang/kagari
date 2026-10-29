//! Generated converters reuse the runtime's checked rooted struct/variant helpers.
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{
    Attribute, Data, DeriveInput, Error, Fields, Ident, LitStr, Result, ext::IdentExt, parse_str,
    token,
};

pub(super) fn expand(input: &DeriveInput) -> Result<TokenStream> {
    if !input.generics.params.is_empty() || input.generics.where_clause.is_some() {
        return Err(Error::new_spanned(
            &input.generics,
            "KagariValue requires a finite nongeneric Rust type; use an explicit adapter for generic factories or borrowed data",
        ));
    }
    let name = input.ident.unraw().to_string();
    let mut script_name = name;
    for attribute in &input.attrs {
        if attribute.path().is_ident("repr") {
            attribute.parse_nested_meta(|meta| {
                if meta.path.is_ident("packed") {
                    return Err(meta.error("KagariValue does not support packed representations"));
                }
                if !meta.input.is_empty() && meta.input.peek(token::Paren) {
                    let content;
                    syn::parenthesized!(content in meta.input);
                    let _: TokenStream = content.parse()?;
                }
                Ok(())
            })?;
        } else if attribute.path().is_ident("kagari") {
            attribute.parse_nested_meta(|meta| {
                if !meta.path.is_ident("name") { return Err(meta.error("unsupported KagariValue attribute; only a short name override is supported")); }
                script_name = meta.value()?.parse::<LitStr>()?.value();
                Ok(())
            })?;
        } else if attribute.path().is_ident("serde") {
            return Err(Error::new_spanned(
                attribute,
                "KagariValue uses the direct Rust representation; serialization attributes require KagariSchema and the explicit Serde adapter",
            ));
        }
    }
    if script_name.contains('#') || parse_str::<Ident>(&script_name).is_err() {
        return Err(Error::new_spanned(
            &input.ident,
            "KagariValue name must be a short script identifier; the registering module owns its namespace",
        ));
    }
    let ident = &input.ident;
    let (schema, encode, decode, projections) = match &input.data {
        Data::Struct(data) => {
            let Fields::Named(fields) = &data.fields else {
                return Err(Error::new_spanned(
                    ident,
                    "KagariValue structs require named fields",
                ));
            };
            let mut schemas = vec![];
            let mut encoders = vec![];
            let mut decoders = vec![];
            let mut projectors = vec![];
            for field in &fields.named {
                reject_attributes(&field.attrs)?;
                let member = field.ident.as_ref().expect("named field");
                let name = member.unraw().to_string();
                let ty = &field.ty;
                schemas.push(quote! { ::kagari_runtime::native::schema::ValueField::new(#name, registration.dependency::<#ty>()?) });
                encoders.push(quote! { encoder.field(#name, self.#member)?; });
                decoders.push(quote! { #member: decoder.field::<#ty>(#name)? });
                projectors.push(quote! {
                    // SAFETY: The derive rejects packed/union/variant fields.
                    // This is a direct initialized inline field; addr_of_mut!
                    // creates no intermediate reference to its ancestor.
                    unsafe {
                        ::kagari_runtime::native::projection::GeneratedField::new::<Self, #ty>(
                            #name, |root| {
                                ::std::ptr::addr_of_mut!((*(root.cast::<Self>())).#member).cast::<()>()
                            },
                        )
                    }
                });
            }
            let encode = if fields.named.is_empty() {
                quote! { cx.write_struct(expected)?.finish() }
            } else {
                quote! { let mut encoder = cx.write_struct(expected)?; #(#encoders)* encoder.finish() }
            };
            let decode = if fields.named.is_empty() {
                quote! { let _decoder = cx.read_struct(expected, value)?; Ok(Self {}) }
            } else {
                quote! { let mut decoder = cx.read_struct(expected, value)?; Ok(Self { #(#decoders),* }) }
            };
            let registration = if fields.named.is_empty() {
                quote! { _registration }
            } else {
                quote! { registration }
            };
            (
                quote! { registration.structure::<Self>(#script_name, |#registration| Ok(vec![#(#schemas),*])) },
                encode,
                decode,
                quote! { Some(vec![#(#projectors),*]) },
            )
        }
        Data::Enum(data) => {
            if data.variants.is_empty() {
                return Err(Error::new_spanned(
                    ident,
                    "KagariValue requires at least one enum variant",
                ));
            }
            let mut schemas = vec![];
            let mut encoders = vec![];
            let mut decoders = vec![];
            for variant in &data.variants {
                reject_attributes(&variant.attrs)?;
                let member = &variant.ident;
                let name = member.unraw().to_string();
                let fields = match &variant.fields {
                    Fields::Unit => vec![],
                    Fields::Unnamed(fields) if fields.unnamed.len() <= 12 => {
                        fields.unnamed.iter().collect()
                    }
                    _ => {
                        return Err(Error::new_spanned(
                            member,
                            "KagariValue enum variants require unit or tuple payloads of at most twelve fields",
                        ));
                    }
                };
                for field in &fields {
                    reject_attributes(&field.attrs)?;
                }
                let types: Vec<_> = fields.iter().map(|field| &field.ty).collect();
                let names: Vec<_> = (0..fields.len())
                    .map(|index| format_ident!("field_{index}"))
                    .collect();
                schemas.push(quote! { ::kagari_runtime::native::schema::ValueVariant::new(#name, [#(registration.dependency::<#types>()?),*]) });
                if matches!(variant.fields, Fields::Unit) {
                    encoders
                        .push(quote! { Self::#member => cx.encode_variant(expected, #name, ()) });
                    decoders.push(quote! { #name => { cx.decode_variant::<()>(expected, value, #name)?; Ok(Self::#member) } });
                } else {
                    encoders.push(quote! { Self::#member(#(#names),*) => cx.encode_variant(expected, #name, (#(#names,)*)) });
                    decoders.push(quote! { #name => {
                        let (#(#names,)*) = cx.decode_variant::<(#(#types,)*)>(expected, value, #name)?;
                        Ok(Self::#member(#(#names),*))
                    } });
                }
            }
            let registration = if data
                .variants
                .iter()
                .any(|variant| !variant.fields.is_empty())
            {
                quote! { registration }
            } else {
                quote! { _registration }
            };
            (
                quote! { registration.enumeration::<Self>(#script_name, |#registration| Ok(vec![#(#schemas),*])) },
                quote! { match self { #(#encoders),* } },
                quote! { match cx.variant_name(expected, value)?.as_str() { #(#decoders,)*
                    _ => Err(::kagari_runtime::error::RuntimeError::module_validation("enum variant differs from the registered Rust representation")),
                } },
                quote! { None },
            )
        }
        Data::Union(_) => {
            return Err(Error::new_spanned(
                ident,
                "KagariValue does not support unions",
            ));
        }
    };
    Ok(quote! {
        impl ::kagari_runtime::native::conversion::KagariType for #ident {
            fn kagari_type(catalog: &::kagari_runtime::native::catalog::DeclarationCatalog) -> ::kagari_runtime::native::binding::NativeResult<::kagari_runtime::native::types::Type> {
                catalog.rust_type::<Self>()
            }

            fn generated_fields() -> Option<Vec<::kagari_runtime::native::projection::GeneratedField>> {
                #projections
            }

            fn register_type(registration: &mut ::kagari_runtime::native::schema::TypeRegistration<'_>) -> ::kagari_runtime::native::binding::NativeResult<::kagari_runtime::native::types::Type> {
                #schema
            }
        }

        impl ::kagari_runtime::native::conversion::IntoKagari for #ident {
            fn into_kagari(self, cx: &mut ::kagari_runtime::native::conversion::context::ConversionContext<'_>, expected: &::kagari_runtime::frame::types::arguments::TypeArgument) -> ::kagari_runtime::native::binding::NativeResult<::kagari_runtime::native::value_handle::ScriptValue> {
                #encode
            }
        }

        impl ::kagari_runtime::native::conversion::FromKagari for #ident {
            fn from_kagari(cx: &mut ::kagari_runtime::native::conversion::context::ConversionContext<'_>, expected: &::kagari_runtime::frame::types::arguments::TypeArgument, value: &::kagari_runtime::native::value_handle::ScriptValue) -> ::kagari_runtime::native::binding::NativeResult<Self> {
                #decode
            }
        }
    })
}

fn reject_attributes(attributes: &[Attribute]) -> Result<()> {
    for attribute in attributes {
        if attribute.path().is_ident("kagari") || attribute.path().is_ident("serde") {
            return Err(Error::new_spanned(
                attribute,
                "KagariValue exports the direct Rust field shape; use KagariSchema with Serde for serialization attributes or an explicit converter",
            ));
        }
    }
    Ok(())
}
