use crate::{hir::FunctionKind, lower::LoweredModule, native::stdlib::invalid};
use kagari_abi::native_import::binding_id;
use kagari_common::cancellation::CancellationToken;
use kagari_stdlib::{NativeMarkerKind, PackageError, ParsedStdlibFile};

pub(super) fn install(
    file: &ParsedStdlibFile,
    lowered: &mut LoweredModule,
    cancel: &CancellationToken,
) -> Result<(), PackageError> {
    for site in file.declarations() {
        cancel.check()?;
        for marker in &site.markers {
            if !matches!(marker.kind, NativeMarkerKind::Native) {
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
            let binding = binding_id(file.source().module_identity(), &marker.binding);
            lowered.native_functions.insert(function.id, binding);
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
