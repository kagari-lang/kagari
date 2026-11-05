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
///
/// An inline module still refers to the original physical file: its source spans
/// are not rebased to zero. Cloning this handle shares the whole parse, rather than
/// copying or detaching the selected body. Borrowed nodes from [`Self::body`] and
/// [`Self::items`] require an owning handle to remain alive.
///
/// ```
/// use kagari_source::source::SourceFile;
/// use kagari_syntax::{ast::views::ItemView, parser::{ParseOptions, parse}};
///
/// let source = SourceFile::new("modules.kgr", "mod child { fn f() {} }");
/// let parsed = parse(&source, ParseOptions::default(), &Default::default())?;
/// let root = parsed.module();
/// let child = root.items().find_map(|item| match item {
///     ItemView::ModuleDef(module) => module.body(),
///     _ => None,
/// }).expect("inline module");
/// let location = child.location();
/// assert!(location.resolve(&parsed.clone()).is_some());
/// let reparsed = parse(&source, ParseOptions::default(), &Default::default())?;
/// assert!(location.resolve(&reparsed).is_none());
/// drop(parsed);
/// drop(root);
/// assert!(child.text().contains("fn f()"));
/// # Ok::<(), kagari_common::cancellation::Cancelled>(())
/// ```
#[derive(Debug, Clone)]
pub struct ModuleSyntax {
    pub(crate) file: Arc<ParsedFile>,
    pub(crate) id: ModuleId,
}

impl ModuleSyntax {
    /// Borrow the selected body; inline bodies have braces, the physical root does not.
    pub fn body(&self) -> Ast<'_, ModuleBody> {
        Ast {
            owner: &self.file,
            node: &self.file.modules[self.id.0 as usize],
        }
    }

    /// Immediate items in authored order, including retained `ItemView::Error` nodes.
    /// Does not descend into nested modules or guarantee that item fields are complete.
    pub fn items(&self) -> impl Iterator<Item = ItemView<'_>> {
        let body = self.body();
        body.node
            .items
            .iter()
            .map(move |item| body.with(item).view())
    }

    /// The complete original physical source, even for a retained inline body.
    pub fn source(&self) -> &SourceFile {
        &self.file.source
    }

    /// Original spelling of [`Self::full_span`], without literal decoding or formatting.
    pub fn text(&self) -> &str {
        let range = self.full_span();
        &self.file.source.text()[range.start..range.end]
    }

    /// Physical UTF-8 byte range of this body and its assigned outer trivia.
    /// The root covers the whole file; an inline body includes its braces, not
    /// the enclosing `mod` keyword/name/attributes.
    pub fn full_span(&self) -> Span {
        if self.id == self.file.root {
            Span::new(0, self.source().text().len())
        } else {
            self.body().full_span()
        }
    }

    /// Physical UTF-8 range inside the inline braces, or the whole file for the root.
    /// When recovery lacks a closing brace, the end is the retained body's end.
    /// This is a lookup extent for [`Self::at_content_range`], not a rebased source.
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

    /// Find the first module whose content range exactly equals `range` in this parse.
    /// Searches the physical module table, including the root. A contained or
    /// overlapping range is insufficient; `None` means no exact body extent matches.
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

    /// Capture a weak, parse-local locator without extending the parse's lifetime.
    pub fn location(&self) -> ModuleLocation {
        ModuleLocation {
            file: Arc::downgrade(&self.file),
            id: self.id,
        }
    }

    /// Join leading `//!` lines in this body's initial trivia with LF separators.
    /// Scanning stops at the first non-trivia token; only the comment marker and
    /// one optional following space are removed. Returns owned text, possibly empty.
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
    /// Includes this body itself, nested module headers and their attributes.
    /// Returning false prunes that node's subtree, not the remaining siblings.
    /// This recursive walk does not collect per-node child vectors or poll cancellation.
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

    /// This body and its logically owned descendants, in source-order preorder.
    /// Uses [`Self::visit_owned`] to eagerly collect a vector before returning the
    /// iterator. Use that visitor to prune or avoid the complete result allocation.
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
    /// Retain a present inline body, including a recovered body with a missing close.
    /// External `mod name;` declarations and missing inline bodies return `None`;
    /// resolving external module files belongs to the package/source layer.
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
/// The weak reference retains neither AST nodes nor source text. This is not a
/// persistent or cross-revision identifier, even when two parses share a SourceFile.
#[derive(Debug, Clone)]
pub struct ModuleLocation {
    file: Weak<ParsedFile>,
    id: ModuleId,
}

impl ModuleLocation {
    /// Resolve against the original parse or one of its clones.
    /// A newly parsed copy of identical source returns `None`; success returns
    /// an owning module handle and therefore retains the physical parse.
    pub fn resolve(&self, parsed: &Parse) -> Option<ModuleSyntax> {
        (Weak::ptr_eq(&self.file, &Arc::downgrade(&parsed.file))
            && (self.id.0 as usize) < parsed.file.modules.len())
        .then(|| ModuleSyntax {
            file: parsed.file.clone(),
            id: self.id,
        })
    }
}
