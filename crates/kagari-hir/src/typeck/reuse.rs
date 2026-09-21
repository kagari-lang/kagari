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
