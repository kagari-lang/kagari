use std::collections::HashMap;

use kagari_common::{Diagnostic, DiagnosticKind};
use kagari_syntax::ast::{self, AstNode, Item};
use smallvec::SmallVec;

use crate::BoxedDiagnosticBuffer;

#[derive(Debug, Clone, Default)]
pub struct NameTable {
    functions: HashMap<String, usize>,
}

impl NameTable {
    pub fn contains_function(&self, name: &str) -> bool {
        self.functions.contains_key(name)
    }
}

pub fn resolve_names(module: &ast::SourceFile) -> Result<NameTable, BoxedDiagnosticBuffer> {
    let mut names = NameTable::default();
    let mut diagnostics = SmallVec::<[Diagnostic; 4]>::new();

    for (index, item) in module.items().enumerate() {
        let Item::FnDef(function) = item else {
            continue;
        };
        let Some(name) = function.name_text() else {
            diagnostics.push(Diagnostic::error(DiagnosticKind::MissingFunctionName));
            continue;
        };
        if names.functions.insert(name.clone(), index).is_some() {
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::DuplicateFunction { name })
                    .with_span(syntax_span(&function)),
            );
        }
    }

    if diagnostics.is_empty() {
        Ok(names)
    } else {
        Err(Box::new(diagnostics))
    }
}

fn syntax_span(node: &impl AstNode) -> kagari_common::Span {
    let range = node.syntax().text_range();
    kagari_common::Span::new(range.start().into(), range.end().into())
}
