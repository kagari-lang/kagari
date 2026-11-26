//! Declaration-only duplicate-name, writeability and irrefutability checks.

use crate::{
    collections::HashSet,
    hir::{ids::PatternId, writeability::Writeability},
    typeck::{BodyTypeEnv, body::BodyChecker},
    types::TypeId,
};
use kagari_source::diagnostic::{Diagnostic, DiagnosticKind};

impl BodyChecker<'_> {
    pub(in crate::typeck::body) fn check_declaration_pattern(
        &mut self,
        pattern: PatternId,
        expected: &TypeId,
        writeability: Writeability,
        require_irrefutable: bool,
        env: &mut BodyTypeEnv,
    ) {
        self.check_pattern(pattern, expected, env);
        if require_irrefutable
            && !self
                .names
                .pattern_is_irrefutable(&self.lowered.module.nodes, pattern)
        {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                    expected: "irrefutable declaration pattern".into(),
                    found: "refutable pattern".into(),
                })
                .with_span(
                    self.lowered
                        .module
                        .pattern(pattern)
                        .origin
                        .diagnostic_span(),
                ),
            );
        }
        let mut names = HashSet::default();
        for (local, name) in self.lowered.module.nodes.pattern_bindings(pattern) {
            // Bare constructor patterns have a syntax local but introduce no binding.
            if !env.locals.contains_key(&local) {
                continue;
            }
            if !names.insert(name) {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                        expected: "each name bound once in a declaration".into(),
                        found: format!("duplicate binding `{name}`"),
                    })
                    .with_span(
                        self.lowered
                            .module
                            .nodes
                            .local(local)
                            .origin
                            .diagnostic_span(),
                    ),
                );
            }
            env.local_writeability.insert(local, writeability);
        }
    }
}
