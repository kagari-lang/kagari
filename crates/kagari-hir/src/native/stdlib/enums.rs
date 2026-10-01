use crate::{
    hir::ty::TypeKind,
    lower::LoweredModule,
    native::{NativeTypeKind, stdlib::invalid},
};
use kagari_abi::standard::surface::StandardEnum;
use kagari_common::cancellation::CancellationToken;
use kagari_stdlib::{
    index::NativeMarkerKind,
    package::{PackageError, ParsedStdlibFile},
};
use serde::{
    Deserialize,
    de::value::{Error as DeserializationError, StrDeserializer},
};

// Runtime discriminants and payload slots are fixed representation hooks. The
// declarations still own their names, generic binders and resolved field types.
fn layout(kind: StandardEnum) -> &'static [(&'static str, Option<usize>)] {
    match kind {
        StandardEnum::Bound => &[
            ("Included", Some(0)),
            ("Excluded", Some(0)),
            ("Unbounded", None),
        ],
        StandardEnum::Option => &[("Some", Some(0)), ("None", None)],
        StandardEnum::Result => &[("Ok", Some(0)), ("Err", Some(1))],
        StandardEnum::Ordering => &[("Less", None), ("Equal", None), ("Greater", None)],
        StandardEnum::TryFromIntError => &[("OutOfRange", None)],
        StandardEnum::Infallible => &[],
        StandardEnum::ParseError => &[
            ("Empty", None),
            ("InvalidDigit", None),
            ("OutOfRange", None),
            ("InvalidRadix", None),
            ("InvalidSyntax", None),
        ],
    }
}

pub(super) fn install(
    file: &ParsedStdlibFile,
    lowered: &mut LoweredModule,
    cancel: &CancellationToken,
) -> Result<(), PackageError> {
    for site in file.declarations() {
        cancel.check()?;
        for marker in &site.markers {
            if marker.kind != NativeMarkerKind::BuiltinEnum {
                continue;
            }
            let kind = StandardEnum::deserialize(StrDeserializer::<DeserializationError>::new(
                &marker.binding,
            ))
            .map_err(|_| {
                invalid(
                    file,
                    marker.span,
                    format!("unknown native enum binding `{}`", marker.binding),
                )
            })?;
            let native = NativeTypeKind::Enum(kind);
            let item = lowered
                .module
                .enums
                .iter()
                .find(|item| lowered.source_map.enum_span(item.id) == site.span)
                .ok_or_else(|| {
                    invalid(
                        file,
                        marker.span,
                        "builtin_enum requires an enum declaration",
                    )
                })?;
            if item.generic_params.len() != native.arity()
                || item.variants.len() != layout(kind).len()
            {
                return Err(invalid(
                    file,
                    marker.span,
                    "native enum generic or variant count does not match its representation",
                ));
            }
            for (variant, (name, parameter)) in item.variants.iter().zip(layout(kind)) {
                let valid_payload = match (variant.payload.as_slice(), parameter) {
                    ([], None) => true,
                    ([ty], Some(index)) => {
                        matches!(&lowered.module.type_ref(*ty).kind, TypeKind::Named(name) if name == &item.generic_params[*index].name)
                    }
                    _ => false,
                };
                if variant.name != *name || !valid_payload {
                    return Err(invalid(
                        file,
                        marker.span,
                        "native enum discriminants or payload slots do not match its representation",
                    ));
                }
            }
            lowered.native_enums.insert(item.id, native);
            lowered
                .native_attributes
                .insert((marker.span.start, marker.span.end));
        }
    }
    Ok(())
}
