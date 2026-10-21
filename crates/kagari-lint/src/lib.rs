//! Six optional first-release rules over one retained HIR analysis.
//! Required language diagnostics remain the responsibility of `check_program`.

pub mod policy;
pub mod registry;
mod rules;

use crate::{policy::Policy, registry::LintId};
use kagari_common::{
    cancellation::{CancellationToken, Cancelled},
    identity::ModuleIdentity,
};
use kagari_hir::analysis::AnalysisSnapshot;
use kagari_source::{
    identity::{FileId, FileSpan},
    presentation::DiagnosticRecord,
};
use std::collections::HashSet;

pub(crate) struct Finding {
    id: LintId,
    span: FileSpan,
    module: ModuleIdentity,
    message: String,
    notes: Vec<String>,
}

/// Lints authored modules of the root package, never dependency/provider source.
/// Consumes HIR-owned eligibility and binding facts without semantic reprocessing.
/// Malformed suppression policy is always an error, independent of lint levels.
pub fn lint(
    snapshot: &AnalysisSnapshot,
    root: FileId,
    policy: &Policy,
    cancel: &CancellationToken,
) -> Result<Vec<DiagnosticRecord>, Cancelled> {
    let Some(root) = snapshot.source(root) else {
        return Ok(Vec::new());
    };
    let package = &root.module_identity().package;
    let mut seen = HashSet::new();
    let mut findings = Vec::new();
    let mut scopes = Vec::new();
    let mut diagnostics = Vec::new();
    for (identity, node) in snapshot.module_graph().modules() {
        cancel.check()?;
        if &snapshot
            .module_graph()
            .catalog
            .definitions
            .module_identity(*identity)
            .expect("graph module")
            .package
            != package
            || !seen.insert(node.file)
        {
            continue;
        }
        let Some(file) = snapshot.file(node.file) else {
            continue;
        };
        if file.result().facts().lowered.is_registered_native_api() {
            continue;
        }
        scopes.extend(policy::scopes(file, &mut diagnostics, cancel)?);
        rules::collect(file, &mut findings, cancel)?;
    }
    diagnostics.extend(policy::apply(policy, &mut scopes, findings, cancel)?);
    diagnostics.sort_by_key(|record| {
        (
            record.span.map(|span| {
                (
                    snapshot
                        .source(span.file)
                        .map(|source| source.name().to_owned()),
                    span.range.start,
                )
            }),
            record.code.clone(),
            record.message.clone(),
        )
    });
    cancel.check()?;
    Ok(diagnostics)
}

#[cfg(test)]
mod tests;
