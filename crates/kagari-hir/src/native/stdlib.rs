//! Trusted installation of the bundled package into ordinary HIR declarations.

use crate::{
    lower::{LoweredModule, lower_module_controlled},
    native::NativeTypeKind,
};
use kagari_common::{cancellation::CancellationToken, span::Span};
use kagari_stdlib::{
    index::NativeMarkerKind,
    package::{PackageError, ParsedStdlibFile, ParsedStdlibPackage},
};
use kagari_syntax::parser::ParseLimits;
use std::sync::Arc;

mod enums;
mod functions;
mod policies;
#[cfg(test)]
mod tests;

#[derive(Debug)]
pub(crate) struct InstalledStdlib {
    pub package: Arc<ParsedStdlibPackage>,
    pub modules: Vec<Arc<LoweredModule>>,
}

impl InstalledStdlib {
    pub fn prepare(limits: ParseLimits, cancel: &CancellationToken) -> Result<Self, PackageError> {
        let package = Arc::new(ParsedStdlibPackage::prepare(limits, cancel)?);
        let mut modules = Vec::with_capacity(package.files().len());
        for file in package.files() {
            cancel.check()?;
            let mut lowered =
                lower_module_controlled(file.source().clone(), &file.parsed().syntax(), cancel);
            install_types(file, &mut lowered, cancel)?;
            enums::install(file, &mut lowered, cancel)?;
            functions::install(file, &mut lowered, cancel)?;
            policies::install(file, &mut lowered, cancel)?;
            lowered.installed_stdlib = Some(package.clone());
            modules.push(Arc::new(lowered));
        }
        cancel.check()?;
        Ok(Self { package, modules })
    }
}

fn invalid(file: &ParsedStdlibFile, span: Span, message: impl Into<String>) -> PackageError {
    PackageError::Annotation {
        uri: file.source().name().into(),
        span,
        message: message.into(),
    }
}

fn install_types(
    file: &ParsedStdlibFile,
    lowered: &mut LoweredModule,
    cancel: &CancellationToken,
) -> Result<(), PackageError> {
    for site in file.declarations() {
        cancel.check()?;
        for marker in &site.markers {
            if marker.kind != NativeMarkerKind::BuiltinType {
                continue;
            }
            let item = lowered
                .module
                .opaque_types
                .iter()
                .find(|item| lowered.source_map.opaque_type_span(item.id) == site.span)
                .ok_or_else(|| {
                    invalid(
                        file,
                        marker.span,
                        "builtin_type requires a top-level opaque type declaration",
                    )
                })?;
            let kind = NativeTypeKind::from_binding(&marker.binding).ok_or_else(|| {
                invalid(
                    file,
                    marker.span,
                    format!("unknown native type binding `{}`", marker.binding),
                )
            })?;
            if item.generic_params.len() != kind.arity() {
                return Err(invalid(
                    file,
                    marker.span,
                    "native type parameter count does not match its storage representation",
                ));
            }
            if item.definition.is_some() || !item.trait_bounds.is_empty() {
                return Err(invalid(
                    file,
                    site.span,
                    "native opaque types cannot define an alias or direct trait bounds; use explicit implementations",
                ));
            }
            lowered.native_types.insert(item.id, kind);
            lowered
                .native_attributes
                .insert((marker.span.start, marker.span.end));
        }
    }
    for item in &lowered.module.opaque_types {
        cancel.check()?;
        if !lowered.native_types.contains_key(&item.id) {
            return Err(invalid(
                file,
                lowered.source_map.opaque_type_span(item.id),
                "opaque type requires an installed native storage binding",
            ));
        }
    }
    Ok(())
}
