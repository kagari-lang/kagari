//! Declaration policy is checked separately from native binding selection.

use crate::{hir::item::function::FunctionKind, lower::LoweredModule, native::stdlib::invalid};
use kagari_abi::callable::MethodPolicy;
use kagari_common::cancellation::CancellationToken;
use kagari_stdlib::{
    index::NativeMarkerKind,
    package::{PackageError, ParsedStdlibFile},
};

pub(super) fn install(
    file: &ParsedStdlibFile,
    lowered: &mut LoweredModule,
    cancel: &CancellationToken,
) -> Result<(), PackageError> {
    for site in file.declarations() {
        cancel.check()?;
        for marker in site
            .markers
            .iter()
            .filter(|marker| marker.kind == NativeMarkerKind::MethodPolicy)
        {
            let function = lowered
                .module
                .functions
                .iter()
                .find(|function| lowered.source_map.function_span(function.id) == site.span)
                .filter(|function| function.kind == FunctionKind::TraitMethod)
                .ok_or_else(|| {
                    invalid(file, marker.span, "method policy requires a trait method")
                })?;
            let policy = match marker.binding.as_str() {
                "Final" => MethodPolicy {
                    override_allowed: false,
                },
                "Overridable" => MethodPolicy::default(),
                _ => return Err(invalid(file, marker.span, "unknown method policy")),
            };
            if !policy.override_allowed
                && function.body.is_none()
                && !lowered.native_functions.contains_key(&function.id)
            {
                return Err(invalid(
                    file,
                    marker.span,
                    "a required method must allow an implementation",
                ));
            }
            lowered.method_policies.insert(function.id, policy);
            lowered
                .native_attributes
                .insert((marker.span.start, marker.span.end));
        }
    }
    Ok(())
}
