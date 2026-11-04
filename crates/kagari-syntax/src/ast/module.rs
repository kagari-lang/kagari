//! Independently retained module views over a single physical typed parse.

use std::{
    ptr,
    sync::{Arc, Weak},
};

use kagari_common::span::Span;
use kagari_source::source::SourceFile;

use crate::{
    ast::{
        borrow::Ast,
        item::{ModuleBody, ModuleDef, ModuleForm, ModuleId},
        site::Required,
        views::ItemView,
        visit::NodeRef,
    },
    parser::{Parse, ParsedFile},
};

/// The sole owning syntax view. Child views retain all physical nodes/text/diagnostics.
#[derive(Debug, Clone)]
pub struct ModuleSyntax {
    pub(crate) file: Arc<ParsedFile>,
    pub(crate) id: ModuleId,
}

impl ModuleSyntax {
    pub fn body(&self) -> Ast<'_, ModuleBody> {
        Ast {
            owner: &self.file,
            node: &self.file.modules[self.id.0 as usize],
        }
    }

    pub fn items(&self) -> impl Iterator<Item = ItemView<'_>> {
        let body = self.body();
        body.node
            .items
            .iter()
            .map(move |item| body.with(item).view())
    }

    pub fn source(&self) -> &SourceFile {
        &self.file.source
    }

    pub fn text(&self) -> &str {
        let range = self.full_span();
        &self.file.source.text()[range.start..range.end]
    }

    pub fn full_span(&self) -> Span {
        if self.id == self.file.root {
            Span::new(0, self.source().text().len())
        } else {
            self.body().full_span()
        }
    }

    pub fn content_range(&self) -> Option<Span> {
        let body = self.body();
        let Some(open) = &body.node.open else {
            return Some(Span::new(0, self.source().text().len()));
        };
        let start = self.file.lexical.span(open.index() as usize).end;
        let end = body
            .node
            .close
            .as_ref()
            .and_then(Required::as_ref)
            .map_or_else(
                || body.body_span().end,
                |close| self.file.lexical.span(close.index() as usize).start,
            );
        Some(Span::new(start, end))
    }

    pub fn at_content_range(parsed: &Parse, range: Span) -> Option<Self> {
        parsed
            .file
            .modules
            .iter()
            .enumerate()
            .map(|(index, _)| Self {
                file: parsed.file.clone(),
                id: ModuleId(index as u32),
            })
            .find(|module| module.content_range() == Some(range))
    }

    pub fn location(&self) -> ModuleLocation {
        ModuleLocation {
            file: Arc::downgrade(&self.file),
            id: self.id,
        }
    }

    pub fn module_documentation(&self) -> String {
        let body = self.body();
        let mut index = body
            .node
            .open
            .as_ref()
            .map_or(0, |open| open.index() as usize + 1);
        let mut lines = Vec::new();
        while self.file.lexical.kind(index).is_trivia() {
            let span = self.file.lexical.span(index);
            let text = &self.source().text()[span.start..span.end];
            if let Some(line) = text.strip_prefix("//!") {
                lines.push(line.strip_prefix(' ').unwrap_or(line));
            }
            index += 1;
        }
        lines.join("\n")
    }

    /// Visits this module's typed fields, excluding independently owned inline bodies.
    pub fn visit_owned<'a>(&'a self, callback: &mut impl FnMut(NodeRef<'a>) -> bool) {
        let root = self.body();
        NodeRef::ModuleBody(root).visit(&mut |node| {
            if let NodeRef::ModuleBody(body) = node
                && !ptr::eq(body.node, root.node)
            {
                return false;
            }
            callback(node)
        });
    }

    pub fn descendants(&self) -> impl Iterator<Item = NodeRef<'_>> {
        let mut nodes = Vec::new();
        self.visit_owned(&mut |node| {
            nodes.push(node);
            true
        });
        nodes.into_iter()
    }
}

impl Ast<'_, ModuleDef> {
    pub fn body(self) -> Option<ModuleSyntax> {
        let ModuleForm::Inline(inline) = &self.node.form else {
            return None;
        };
        let Required::Present(id) = inline.body else {
            return None;
        };
        Some(ModuleSyntax {
            file: self.owner.clone(),
            id,
        })
    }
}

/// A private body ID tied to its exact parse allocation. A foreign parse cannot resolve it.
#[derive(Debug, Clone)]
pub struct ModuleLocation {
    file: Weak<ParsedFile>,
    id: ModuleId,
}

impl ModuleLocation {
    pub fn resolve(&self, parsed: &Parse) -> Option<ModuleSyntax> {
        (Weak::ptr_eq(&self.file, &Arc::downgrade(&parsed.file))
            && (self.id.0 as usize) < parsed.file.modules.len())
        .then(|| ModuleSyntax {
            file: parsed.file.clone(),
            id: self.id,
        })
    }
}
