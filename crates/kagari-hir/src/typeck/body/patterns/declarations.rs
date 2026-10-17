//! Declaration-only duplicate-name, writeability and irrefutability checks.

use crate::{
    hir::{ids::PatternId, writeability::Writeability},
    typeck::{BodyTypeEnv, body::BodyChecker},
    types::TypeId,
};
use kagari_source::diagnostic::{Diagnostic, DiagnosticKind};

use std::collections::HashSet;

impl BodyChecker<'_> {
    pub(in crate::typeck::body) fn check_declaration_pattern(
        &mut self,
        pattern: PatternId,
        expected: &TypeId,
        writeability: Writeability,
        env: &mut BodyTypeEnv,
    ) {
        self.check_pattern(pattern, expected, env);
        if !self
            .names
            .pattern_is_irrefutable(&self.lowered.module, pattern)
        {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                    expected: "irrefutable declaration pattern".into(),
                    found: "refutable pattern".into(),
                })
                .with_span(self.lowered.source_map.pattern_span(pattern)),
            );
        }
        let mut names = HashSet::new();
        for (local, name) in self.lowered.module.body.pattern_bindings(pattern) {
            if !names.insert(name) {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                        expected: "each name bound once in a declaration".into(),
                        found: format!("duplicate binding `{name}`"),
                    })
                    .with_span(self.lowered.source_map.local_span(local)),
                );
            }
            env.local_writeability.insert(local, writeability);
        }
    }
}
