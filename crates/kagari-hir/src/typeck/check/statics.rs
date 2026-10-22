//! Static signatures and ordinary runtime initializer checking, without const evaluation.

use crate::{
    DiagnosticBuffer,
    declarations::Declarations,
    hir::ids::StaticId,
    lower::LoweredModule,
    resolver::resolved::ResolvedNames,
    typeck::{
        BodyTypeEnv, TypeIndexes,
        body::BodyChecker,
        completion,
        table::TypeTable,
        ty::{display_type, display_type_id, resolve_type},
    },
    types::TypeId,
};
use kagari_common::cancellation::CancellationToken;
use kagari_source::diagnostic::{Diagnostic, DiagnosticKind, Severity};
use std::collections::{HashMap, HashSet};

/// Resolve every explicit annotation before any initializer is visited.
pub(super) fn signatures(
    lowered: &LoweredModule,
    declarations: &Declarations,
    table: &mut TypeTable,
    diagnostics: &mut DiagnosticBuffer,
    cancel: &CancellationToken,
) -> HashMap<StaticId, TypeId> {
    let mut result = HashMap::new();
    for item in &lowered.module.statics {
        if cancel.check().is_err() {
            break;
        }
        let ty = if let Some(annotation) = item.ty {
            let ty = resolve_type(&lowered.module, annotation, declarations, table, cancel);
            if ty.is_unresolved() {
                diagnostics.push(
                    Diagnostic::error(DiagnosticKind::UnknownTypeAnnotation {
                        type_name: display_type(&lowered.module, annotation),
                    })
                    .with_span(lowered.source_map.type_span(annotation)),
                );
            }
            ty
        } else {
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::ExpectedType)
                    .with_span(lowered.source_map.static_span(item.id)),
            );
            TypeId::Error
        };
        result.insert(item.id, ty);
    }
    result
}

pub(super) fn initializers(
    lowered: &LoweredModule,
    names: &ResolvedNames,
    indexes: TypeIndexes<'_>,
    table: &mut TypeTable,
    diagnostics: &mut DiagnosticBuffer,
) -> HashSet<StaticId> {
    let shared_valid = !diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Error);
    let mut valid = HashSet::new();
    for item in &lowered.module.statics {
        if indexes.cancel.check().is_err() {
            break;
        }
        let diagnostic_start = diagnostics.len();
        let expected = indexes
            .top_level_index
            .statics
            .get(&item.id)
            .cloned()
            .unwrap_or(TypeId::Error);
        let mut checker = BodyChecker::new(
            lowered,
            names,
            indexes,
            diagnostics,
            table,
            &item.name,
            expected.clone(),
        );
        let actual = checker.solve_initializer(
            item.initializer,
            &mut BodyTypeEnv::default(),
            Some(&expected),
        );
        if completion::expr_can_complete(
            &lowered.module,
            names,
            table,
            item.initializer,
            indexes.cancel,
        )
        .unwrap_or(false)
            && actual.conflicts_with(&expected)
        {
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::AssignmentTypeMismatch {
                    expected: display_type_id(indexes.declarations.definitions(), &expected),
                    found: display_type_id(indexes.declarations.definitions(), &actual),
                })
                .with_span(lowered.source_map.expr_span(item.initializer)),
            );
        }
        if shared_valid
            && indexes.cancel.check().is_ok()
            && !diagnostics[diagnostic_start..]
                .iter()
                .any(|diagnostic| diagnostic.severity == Severity::Error)
        {
            valid.insert(item.id);
        }
    }
    valid
}
