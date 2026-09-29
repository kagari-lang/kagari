use crate::{
    hir::FunctionKind,
    lower::LoweredModule,
    native::{EngineNativeBinding, stdlib::invalid},
};
use kagari_abi::standard::{
    StandardIntrinsic,
    bindings::{NativeDefaultMethod, NativeProtocolMethod},
};
use kagari_common::{cancellation::CancellationToken, integer::IntegerMethod};
use kagari_stdlib::{NativeMarkerKind, PackageError, ParsedStdlibFile};
use serde::{
    Deserialize,
    de::value::{Error as DeserializationError, StrDeserializer},
};

fn binding<T: for<'de> Deserialize<'de>>(text: &str) -> Option<T> {
    T::deserialize(StrDeserializer::<DeserializationError>::new(text)).ok()
}

#[cfg(test)]
#[test]
fn native_binding_names_are_closed_and_exact() {
    assert_eq!(
        binding::<NativeDefaultMethod>("IteratorMap"),
        Some(NativeDefaultMethod::Map)
    );
    assert_eq!(
        binding::<NativeDefaultMethod>("ListJoin"),
        Some(NativeDefaultMethod::ListJoin)
    );
    assert_eq!(binding::<NativeDefaultMethod>("IteratorListJoin"), None);
    assert_eq!(binding::<NativeDefaultMethod>("Map"), None);
    assert_eq!(binding::<StandardIntrinsic>("ArraySortUnknown"), None);
    assert_eq!(
        binding::<NativeProtocolMethod>("CollectionIter"),
        Some(NativeProtocolMethod::CollectionIter)
    );
}

pub(super) fn install(
    file: &ParsedStdlibFile,
    lowered: &mut LoweredModule,
    cancel: &CancellationToken,
) -> Result<(), PackageError> {
    for site in file.declarations() {
        cancel.check()?;
        for marker in &site.markers {
            if !matches!(
                marker.kind,
                NativeMarkerKind::Intrinsic
                    | NativeMarkerKind::Numeric
                    | NativeMarkerKind::ParseRadix
            ) {
                continue;
            }
            let function = lowered
                .module
                .functions
                .iter()
                .find(|function| lowered.source_map.function_span(function.id) == site.span)
                .ok_or_else(|| {
                    invalid(
                        file,
                        marker.span,
                        "native function annotation requires a function or method",
                    )
                })?;
            if function.body.is_some() {
                return Err(invalid(
                    file,
                    marker.span,
                    "a native function cannot also have a script body",
                ));
            }
            let kind = match marker.kind {
                NativeMarkerKind::Intrinsic if function.kind == FunctionKind::TraitMethod => {
                    binding::<NativeDefaultMethod>(&marker.binding)
                        .map(EngineNativeBinding::TraitDefault)
                }
                NativeMarkerKind::Intrinsic => binding::<StandardIntrinsic>(&marker.binding)
                    .map(EngineNativeBinding::Intrinsic)
                    .or_else(|| {
                        (function.kind == FunctionKind::ImplMethod)
                            .then(|| {
                                binding::<NativeProtocolMethod>(&marker.binding)
                                    .map(EngineNativeBinding::Protocol)
                            })
                            .flatten()
                    }),
                NativeMarkerKind::Numeric if function.kind == FunctionKind::ImplMethod => {
                    binding::<IntegerMethod>(&marker.binding).map(EngineNativeBinding::Integer)
                }
                NativeMarkerKind::ParseRadix
                    if function.kind == FunctionKind::ImplMethod
                        && marker.binding == "ParseRadix" =>
                {
                    Some(EngineNativeBinding::ParseRadix)
                }
                _ => None,
            }
            .ok_or_else(|| {
                invalid(
                    file,
                    marker.span,
                    format!(
                        "unknown or misplaced native function binding `{}`",
                        marker.binding
                    ),
                )
            })?;
            let operand_count = match kind {
                EngineNativeBinding::Intrinsic(intrinsic) => Some(intrinsic.operand_count()),
                EngineNativeBinding::Integer(_) | EngineNativeBinding::ParseRadix => Some(2),
                EngineNativeBinding::TraitDefault(_) | EngineNativeBinding::Protocol(_) => None,
            };
            if let Some(expected) = operand_count
                && function.params.len() != expected
            {
                return Err(invalid(
                    file,
                    marker.span,
                    format!(
                        "native function binding `{}` parameter count mismatch: expected {expected}, found {}",
                        marker.binding,
                        function.params.len(),
                    ),
                ));
            }
            lowered.native_functions.insert(function.id, kind);
            lowered
                .native_attributes
                .insert((marker.span.start, marker.span.end));
        }
    }
    for function in &lowered.module.functions {
        cancel.check()?;
        if function.body.is_none()
            && function.kind != FunctionKind::TraitMethod
            && !lowered.native_functions.contains_key(&function.id)
        {
            return Err(invalid(
                file,
                lowered.source_map.function_span(function.id),
                "function without a body requires an installed native binding",
            ));
        }
    }
    for item in &mut lowered.module.traits {
        for method in &mut item.methods {
            if lowered.native_functions.contains_key(&method.function) {
                method.has_default = true;
            }
        }
    }
    Ok(())
}
