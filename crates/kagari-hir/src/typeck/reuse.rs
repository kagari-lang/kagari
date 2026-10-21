//! Cache semantic facts by source content, remapping every arena ID on reuse.

use crate::{
    hir::{
        ids::FunctionId,
        item::function::{Function, FunctionKind},
    },
    lower::LoweredModule,
    typeck::table::TypeTable,
};

use kagari_common::span::Span;
use kagari_source::diagnostic::Diagnostic;
use kagari_syntax::{lexer, token::Token};
use std::collections::HashSet;

/// Prior body facts plus old/new text used to attempt arena-aware reuse.
///
/// The caller first checks imports, signatures and aggregate environments. Reuse
/// then requires compatible surrounding declaration tokens, unchanged body text
/// and a source-map correspondence. Local IDs are remapped into the new arena;
/// old IDs cannot simply be copied. Failed correspondence falls back to checking.
pub struct BodyReuse<'a> {
    /// Positive eligibility evidence retained before diagnostic truncation.
    pub previous_valid_bodies: &'a HashSet<FunctionId>,
    /// Previous analysis diagnostics used to exclude unsafe reuse.
    pub previous_diagnostics: &'a [Diagnostic],
    /// Old source, nodes and source-map ownership for remapping.
    pub previous_lowered: &'a LoweredModule,
    /// Previously published type/call/member facts.
    pub previous_types: &'a TypeTable,
    /// Text whose byte ranges the previous source map describes.
    pub old_text: &'a str,
    /// Current text whose byte ranges the new lowering describes.
    pub new_text: &'a str,
}

impl BodyReuse<'_> {
    /// Requires compatible surrounding source tokens and diagnostic locations before trying body reuse.
    pub(crate) fn environment_matches(&self, current: &LoweredModule) -> bool {
        if self
            .previous_lowered
            .module
            .functions
            .iter()
            .chain(&current.module.functions)
            .any(|f| matches!(f.kind, FunctionKind::Local { .. }))
        {
            return false;
        }
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
                .zip(environment(current, self.new_text))
                .is_some_and(|(old, new)| same_environment_tokens(&old, &new))
    }

    /// Matches unchanged body syntax and remaps its IDs/spans into the new lowering before restoring facts.
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
        if !self.previous_valid_bodies.contains(&old.id) {
            return false;
        }
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

// Compare token boundaries and literal contents, never concatenated spellings:
// dropping trivia must not turn `a b` into `ab` or alter a string literal.
fn same_environment_tokens(old: &str, new: &str) -> bool {
    let old_tokens = lexer::lex(old);
    let new_tokens = lexer::lex(new);
    let significant = |token: &&Token| !token.kind.is_trivia();
    old_tokens
        .iter()
        .filter(significant)
        .map(|token| (&token.kind, &old[token.span.start..token.span.end]))
        .eq(new_tokens
            .iter()
            .filter(significant)
            .map(|token| (&token.kind, &new[token.span.start..token.span.end])))
}

fn environment(module: &LoweredModule, text: &str) -> Option<String> {
    let mut bodies = module
        .module
        .functions
        .iter()
        .filter(|f| matches!(f.kind, FunctionKind::User | FunctionKind::ImplMethod))
        .filter_map(|f| f.body.map(|body| module.source_map.block_span(body)))
        .collect::<Vec<_>>();
    bodies.sort_by_key(|span| span.start);
    let mut result = String::new();
    let extent = module
        .source
        .inline_range()
        .unwrap_or(Span::new(0, text.len()));
    text.get(extent.start..extent.end)?;
    let mut cursor = extent.start;
    for span in bodies {
        if span.start < cursor || span.end > extent.end {
            return None;
        }
        result.push_str(&text[cursor..span.start]);
        result.push_str("{}");
        cursor = span.end;
    }
    result.push_str(text.get(cursor..extent.end)?);
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kagari_common::span::Span;
    use kagari_source::{
        diagnostic::{Diagnostic, DiagnosticKind},
        source::SourceFile,
    };
    use kagari_stdlib::catalog as foundation_catalog;

    #[test]
    fn inline_environment_uses_only_its_logical_extent() {
        use crate::lower::lower_module_controlled;
        use kagari_syntax::{
            ast::{item::Item, module::ModuleSyntax},
            parser::parse,
        };
        use std::sync::Arc;
        fn lower(text: &str) -> LoweredModule {
            let source = SourceFile::new("inline.kgr", text);
            let parsed = parse(&source);
            let Item::ModuleDef(child) = parsed
                .syntax()
                .items()
                .find(|item| matches!(item, Item::ModuleDef(_)))
                .unwrap()
            else {
                panic!()
            };
            let view = ModuleSyntax::from(child.block().unwrap());
            let source = SourceFile::inline_module(
                &source,
                "a",
                kagari_source::identity::FileId::fresh(),
                view.content_range().unwrap(),
            );
            lower_module_controlled(Arc::new(source), &view, &Default::default())
        }
        let old = lower("const unrelated: i32 = 1; mod a { fn f() -> i32 { 1 } fn g() {} }");
        let new =
            lower("const unrelated: bool = true; mod a { fn f() -> i32 { 2 + 3 } fn g() {} }");
        let old = environment(&old, old.source.text()).unwrap();
        let new = environment(&new, new.source.text()).unwrap();
        assert!(same_environment_tokens(&old, &new));
        assert!(!old.contains("unrelated"));
    }

    #[test]
    fn environment_tokens_ignore_only_trivia() {
        assert!(same_environment_tokens(
            "const label: String = \"// 中 😀\";",
            "// moved 中 😀\r\nconst label : String = \"// 中 😀\" ;"
        ));
        for (old, new) in [
            ("a b", "ab"),
            ("1 2", "12"),
            ("\"a b\"", "\"ab\""),
            ("\"// a\"", "\"// b\""),
            ("val value: i32", "val value: bool"),
        ] {
            assert!(!same_environment_tokens(old, new), "{old} vs {new}");
        }
    }

    #[test]
    fn reuse_distinguishes_global_point_and_neighbor_diagnostics() {
        let source = SourceFile::new(
            "reuse.kgr",
            "const global: i32 = 0; fn one() -> i32 { 1 } fn two() -> i32 { 2 }",
        );
        let analysis = crate::analyze_source(&source, foundation_catalog::shared().unwrap())
            .expect("installed declaration analysis");
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
            let span = facts.lowered.source_map.block_span(function.body.unwrap());
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
                previous_valid_bodies: &facts.typed.valid_bodies,
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
