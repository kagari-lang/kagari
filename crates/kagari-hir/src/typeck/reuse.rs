//! Cache semantic facts by source content, remapping every arena ID on reuse.
use super::TypeTable;
use crate::{
    hir::{Function, FunctionKind},
    lower::LoweredModule,
};

pub struct BodyReuse<'a> {
    pub previous_diagnostics: &'a [kagari_common::Diagnostic],
    pub previous_lowered: &'a LoweredModule,
    pub previous_types: &'a TypeTable,
    pub old_text: &'a str,
    pub new_text: &'a str,
}

impl BodyReuse<'_> {
    pub(crate) fn environment_matches(&self, current: &LoweredModule) -> bool {
        self.previous_diagnostics.iter().all(|diagnostic| {
            diagnostic.span.is_some_and(|span| {
                self.previous_lowered
                    .module
                    .functions
                    .iter()
                    .any(|function| {
                        let owner = self.previous_lowered.source_map.function_span(function.id);
                        matches!(function.kind, FunctionKind::User | FunctionKind::ImplMethod)
                            && owner.start <= span.start
                            && span.end <= owner.end
                    })
            })
        }) && self.previous_lowered.source.module_identity() == current.source.module_identity()
            && environment(self.previous_lowered, self.old_text)
                == environment(current, self.new_text)
    }
    pub(crate) fn restore(
        &self,
        current: &LoweredModule,
        function: &Function,
        table: &mut TypeTable,
    ) -> bool {
        if !matches!(function.kind, FunctionKind::User | FunctionKind::ImplMethod) {
            return false;
        }
        let Some(old) = self
            .previous_lowered
            .module
            .functions
            .iter()
            .find(|old| old.kind == function.kind && old.id == function.id)
        else {
            return false;
        };
        let old_span = self.previous_lowered.source_map.function_span(old.id);
        for diagnostic in self.previous_diagnostics {
            let Some(span) = diagnostic.span else {
                return false;
            };
            if (span.start < old_span.end && old_span.start < span.end)
                || (span.start == span.end
                    && old_span.start <= span.start
                    && span.start <= old_span.end)
            {
                return false;
            }
        }
        let new_span = current.source_map.function_span(function.id);
        let Some(old_text) = self.old_text.get(old_span.start..old_span.end) else {
            return false;
        };
        if self.new_text.get(new_span.start..new_span.end) != Some(old_text) {
            return false;
        }
        table.restore_function(
            self.previous_types,
            &self.previous_lowered.source_map,
            &current.source_map,
            old_span,
            new_span,
        )
    }
}

fn environment(module: &LoweredModule, text: &str) -> String {
    let mut bodies = module
        .module
        .functions
        .iter()
        .filter(|f| matches!(f.kind, FunctionKind::User | FunctionKind::ImplMethod))
        .map(|f| module.source_map.block_span(f.body))
        .collect::<Vec<_>>();
    bodies.sort_by_key(|span| span.start);
    let mut result = String::new();
    let mut cursor = 0;
    for span in bodies {
        if span.start < cursor || span.end > text.len() {
            return text.to_owned();
        }
        result.push_str(&text[cursor..span.start]);
        result.push_str("{}");
        cursor = span.end;
    }
    result.push_str(&text[cursor..]);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use kagari_common::{Diagnostic, DiagnosticKind, SourceFile, Span};

    #[test]
    fn reuse_distinguishes_global_point_and_neighbor_diagnostics() {
        let source = SourceFile::new(
            "reuse.kgr",
            "const global: i32 = 0; fn one() -> i32 { 1 } fn two() -> i32 { 2 }",
        );
        let analysis = crate::analyze_source(&source, Default::default());
        assert!(analysis.diagnostics().is_empty());
        let facts = analysis.facts();
        let one = facts
            .lowered
            .module
            .functions
            .iter()
            .find(|f| f.name == "one")
            .unwrap();
        let two = facts
            .lowered
            .module
            .functions
            .iter()
            .find(|f| f.name == "two")
            .unwrap();
        let point = |function: &Function| {
            let span = facts.lowered.source_map.block_span(function.body);
            Span {
                start: span.start + 1,
                end: span.start + 1,
            }
        };
        for (span, environment_valid, one_reusable) in [
            (None, false, false),
            (Some(Span { start: 0, end: 5 }), false, false),
            (Some(point(one)), true, false),
            (Some(point(two)), true, true),
        ] {
            let diagnostic = Diagnostic {
                span,
                ..Diagnostic::error(DiagnosticKind::ExpectedExpression)
            };
            let diagnostics = [diagnostic];
            let reuse = BodyReuse {
                previous_diagnostics: &diagnostics,
                previous_lowered: &facts.lowered,
                previous_types: &facts.typed.type_table,
                old_text: source.text(),
                new_text: source.text(),
            };
            assert_eq!(
                reuse.environment_matches(&facts.lowered),
                environment_valid,
                "{span:?}"
            );
            if environment_valid {
                assert_eq!(
                    reuse.restore(&facts.lowered, one, &mut TypeTable::default()),
                    one_reusable,
                    "{span:?}"
                );
            }
        }
    }
}
