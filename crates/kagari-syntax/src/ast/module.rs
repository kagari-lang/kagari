//! Logical module views over one retained physical syntax tree.

use crate::{
    ast::{
        item::{Item, ModuleBlock, SourceFile},
        support,
        traits::AstNode,
    },
    kind::SyntaxKind,
    syntax_node::{KagariLanguage, SyntaxNode},
};
use kagari_common::span::Span;
use rowan::{NodeOrToken, TextRange, TextSize, WalkEvent, ast::SyntaxNodePtr};
use std::iter;

/// A physical root or an attached inline body, with original byte offsets.
///
/// For `mod a { fn f() {} mod b {} }`, the inline view of `a` yields `f`
/// and the declaration of `b`. Its owned traversal excludes the body of `b`.
/// A view retains its tree, but carries no file identity, revision or semantic IDs.
#[derive(Debug, Clone)]
pub enum ModuleSyntax {
    Source(SourceFile),
    Inline(ModuleBlock),
}

impl ModuleSyntax {
    /// Borrows the attached root/block node, without rebasing its coordinates.
    pub fn syntax(&self) -> &SyntaxNode {
        match self {
            Self::Source(root) => root.syntax(),
            Self::Inline(block) => block.syntax(),
        }
    }

    /// Finds a logical view by its physical content extent in this exact tree.
    /// Returns `None` for out-of-bounds or non-module ranges. This is useful when
    /// a retained inline source is submitted independently of its parent analysis.
    pub fn at_content_range(root: &SourceFile, range: Span) -> Option<Self> {
        let view = Self::Source(root.clone());
        if view.content_range() == Some(range) {
            return Some(view);
        }
        let start = TextSize::from(u32::try_from(range.start).ok()?);
        let end = TextSize::from(u32::try_from(range.end).ok()?);
        if start > end {
            return None;
        }
        let range_node = TextRange::new(start, end);
        if !root.syntax().text_range().contains_range(range_node) {
            return None;
        }
        let node = match root.syntax().covering_element(range_node) {
            NodeOrToken::Node(node) => node,
            NodeOrToken::Token(token) => token.parent()?,
        };
        node.ancestors()
            .filter_map(ModuleBlock::cast)
            .map(Self::Inline)
            .find(|view| view.content_range() == Some(range))
    }

    /// Iterates immediate declarations, including child module headers.
    pub fn items(&self) -> impl Iterator<Item = Item> {
        support::children(self.syntax())
    }

    /// Visits this module's nodes while excluding nested inline bodies.
    ///
    /// Includes the current root and ordinary function/trait/impl contents.
    /// Attributes on a nested module header belong to this traversal.
    pub fn descendants(&self) -> impl Iterator<Item = SyntaxNode> {
        let root = self.syntax().clone();
        let mut walk = root.preorder();
        iter::from_fn(move || {
            while let Some(event) = walk.next() {
                if let WalkEvent::Enter(node) = event {
                    if node != root && node.kind() == SyntaxKind::ModuleBlock {
                        walk.skip_subtree();
                    } else {
                        return Some(node);
                    }
                }
            }
            None
        })
    }

    /// Returns the physical content extent, excluding present inline braces.
    ///
    /// Missing closing braces retain content through the recovered node's end.
    /// Missing opening braces yield `None`, rather than a fabricated child body.
    pub fn content_range(&self) -> Option<Span> {
        let range = self.syntax().text_range();
        match self {
            Self::Source(_) => Some(Span::new(range.start().into(), range.end().into())),
            Self::Inline(block) => {
                let open = support::token(block.syntax(), SyntaxKind::LBrace)?;
                let end = support::token(block.syntax(), SyntaxKind::RBrace)
                    .map_or(range.end(), |close| close.text_range().start());
                Some(Span::new(open.text_range().end().into(), end.into()))
            }
        }
    }

    /// Reads leading inner documentation inside this logical module.
    pub fn module_documentation(&self) -> String {
        match self {
            Self::Source(root) => root.module_documentation(),
            Self::Inline(block) => block
                .syntax()
                .children_with_tokens()
                .skip_while(|token| token.kind() == SyntaxKind::LBrace)
                .take_while(|element| element.kind().is_trivia())
                .filter_map(|element| element.into_token())
                .filter_map(|token| {
                    token
                        .text()
                        .strip_prefix("//!")
                        .map(|line| line.strip_prefix(' ').unwrap_or(line).to_owned())
                })
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }

    /// Captures a locator that must be paired with this view's physical parse.
    pub fn location(&self) -> ModuleLocation {
        ModuleLocation(SyntaxNodePtr::new(self.syntax()))
    }
}

impl From<SourceFile> for ModuleSyntax {
    fn from(root: SourceFile) -> Self {
        Self::Source(root)
    }
}

impl From<ModuleBlock> for ModuleSyntax {
    fn from(block: ModuleBlock) -> Self {
        Self::Inline(block)
    }
}

/// A root/block locator, meaningful only in its retained physical parse.
///
/// This contains no red cursor and can be stored in thread-safe snapshots.
/// It is not a persistent module identity or proof that two parses match.
#[derive(Debug, Clone, Copy)]
pub struct ModuleLocation(SyntaxNodePtr<KagariLanguage>);

impl ModuleLocation {
    /// Resolves against the owning root, returning `None` for a missing node.
    /// Callers must retain the original parse; range equality alone is not ownership.
    pub fn resolve(&self, root: &SourceFile) -> Option<ModuleSyntax> {
        let node = self.0.try_to_node(root.syntax())?;
        match node.kind() {
            SyntaxKind::SourceFile => SourceFile::cast(node).map(ModuleSyntax::Source),
            SyntaxKind::ModuleBlock => ModuleBlock::cast(node).map(ModuleSyntax::Inline),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use kagari_source::source::SourceFile as Source;

    #[test]
    fn attached_module_views_keep_offsets_recovery_and_ownership() {
        fn thread_safe<T: Send + Sync>() {}
        thread_safe::<ModuleLocation>();
        let text = "// 中文😀\r\nmod a { //! A\r\nfn f() {} mod b { fn g() {} } }";
        let parsed = parse(&Source::new("modules.kgr", text));
        let root = parsed.syntax();
        let Item::ModuleDef(module) = root.items().next().unwrap() else {
            panic!()
        };
        let view = ModuleSyntax::Inline(module.block().unwrap());
        assert_eq!(view.module_documentation(), "A");
        assert_eq!(view.items().count(), 2);
        assert_eq!(
            view.descendants()
                .filter(|n| n.kind() == SyntaxKind::FnDef)
                .count(),
            1
        );
        let restored = view.location().resolve(&root).unwrap();
        assert_eq!(restored.syntax(), view.syntax());
        assert!(restored.syntax().parent().is_some());
        assert_eq!(
            view.content_range().unwrap().start,
            text.find('{').unwrap() + 1
        );
        for text in ["mod a { fn last() {}", "mod a {", "mod a fn last() {}"] {
            let parsed = parse(&Source::new("broken.kgr", text));
            assert!(!parsed.diagnostics().is_empty());
            let Item::ModuleDef(module) = parsed.syntax().items().next().unwrap() else {
                panic!()
            };
            let view = ModuleSyntax::Inline(module.block().unwrap());
            if let Some(range) = view.content_range() {
                assert_eq!(range.end, text.len());
                assert_eq!(view.items().count(), usize::from(text.contains("fn last")));
            } else {
                assert!(!text.contains("mod a {"));
            }
        }
    }
}
