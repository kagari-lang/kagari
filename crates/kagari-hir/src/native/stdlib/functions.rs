use crate::{hir::FunctionKind, lower::LoweredModule, native::stdlib::invalid};
use kagari_common::cancellation::CancellationToken;
use kagari_stdlib::{NativeMarkerKind, PackageError, ParsedStdlibFile};
use kagari_stdlib_provider::contracts;
use std::{collections::HashMap, sync::Arc};

pub(super) fn install(
    file: &ParsedStdlibFile,
    lowered: &mut LoweredModule,
    cancel: &CancellationToken,
) -> Result<(), PackageError> {
    let contracts: HashMap<_, _> = contracts().into_iter().collect();
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
            let contract = contracts.get(marker.binding.as_str()).ok_or_else(|| {
                invalid(
                    file,
                    marker.span,
                    format!("native provider has no binding `{}`", marker.binding),
                )
            })?;
            lowered
                .native_functions
                .insert(function.id, Arc::new(contract.clone()));
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
