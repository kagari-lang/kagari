//! Check the typed field surface of structured Host declarations against their adopted owner.
use crate::{
    DiagnosticBuffer,
    analysis::error::AnalysisError,
    declarations::Declarations,
    lower::{LoweredModule, ModuleInputKind},
    resolver::resolved::ResolvedName,
    typeck::ModuleSignatures,
    types::semantic::lower_type,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{metadata::DefinitionMetadata, table::DefinitionId},
};
use kagari_source::diagnostic::{Diagnostic, DiagnosticKind};
use kagari_types::declaration::{
    FieldDef, TypeDefKind,
    module::{DeclarationError, ModuleDecl},
};
use std::collections::BTreeMap;

/// The source surface must describe the same Host fields as the static schema.
/// Execution paths do not grant field selection or mutation rights.
pub(crate) fn validate_host_surfaces(
    lowered: &LoweredModule,
    signatures: &ModuleSignatures,
    declarations: &Declarations,
    surfaces: &HostSurfaces<'_>,
    diagnostics: &mut DiagnosticBuffer,
    cancel: &CancellationToken,
) {
    if lowered.input_kind != ModuleInputKind::Structured {
        return;
    }
    for item in &lowered.module.native_types {
        if cancel.check().is_err() {
            return;
        }
        if !item.is_host {
            continue;
        }
        let Some(&fields) = declarations
            .definition(ResolvedName::NativeType(item.id))
            .and_then(|id| surfaces.get(id))
        else {
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::InvalidHostPath {
                    reason: format!("missing Host field surface for {}", item.name),
                })
                .with_span(item.origin.diagnostic_span()),
            );
            continue;
        };
        let valid = item.fields.len() == fields.len()
            && item.fields.iter().zip(fields).all(|(actual, expected)| {
                actual.name == expected.name
                    && actual.visibility == expected.visibility
                    && actual.writeability.is_var() == expected.mutable
                    && signatures
                        .type_table()
                        .field_type(actual.id)
                        .is_some_and(|ty| !ty.is_unresolved() && lower_type(&ty) == expected.ty)
            });
        if !valid {
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::InvalidHostPath {
                    reason: format!(
                        "registered Host {} fields differ from its static schema",
                        item.name
                    ),
                })
                .with_span(
                    lowered.module.native_types[item.id.index()]
                        .origin
                        .diagnostic_span(),
                ),
            );
        }
    }
}

/// Borrow only fields for this signature query; adapters and paths stay with registration.
pub(crate) type HostSurfaces<'a> = BTreeMap<DefinitionId, &'a [FieldDef]>;

pub(super) fn collect<'a>(
    registrations: impl Iterator<Item = &'a DefinitionMetadata<ModuleDecl>>,
    cancel: &CancellationToken,
) -> Result<HostSurfaces<'a>, AnalysisError> {
    let mut fields = BTreeMap::new();
    for provider in registrations {
        let module = provider.records();
        for ty in &module.types {
            cancel.check()?;
            if matches!(ty.kind, TypeDefKind::Host(_)) {
                let id = module.definition(
                    ty.kind.definition_kind(),
                    &ty.name,
                    provider.definitions(),
                )?;
                if fields.insert(id, ty.fields.as_slice()).is_some() {
                    return Err(
                        DeclarationError("duplicate registered Host declaration".into()).into(),
                    );
                }
            }
        }
    }
    Ok(fields)
}
