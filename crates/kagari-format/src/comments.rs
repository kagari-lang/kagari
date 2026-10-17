//! Comment attachment is derived from original token neighbors, independent of width.

use kagari_common::cancellation::{CancellationToken, Cancelled};
use kagari_syntax::{
    kind::SyntaxKind,
    syntax_node::{SyntaxNode, SyntaxToken},
};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Attachment {
    Leading,
    Trailing,
    Dangling,
}

pub(crate) struct Comments(BTreeMap<usize, Attachment>);

impl Comments {
    pub fn collect(
        root: &SyntaxNode,
        source: &str,
        cancel: &CancellationToken,
    ) -> Result<Self, Cancelled> {
        let tokens: Vec<_> = root
            .descendants_with_tokens()
            .filter_map(|element| element.into_token())
            .filter(|token| token.kind() != SyntaxKind::Whitespace)
            .collect();
        let mut attachments = BTreeMap::new();
        for (index, token) in tokens.iter().enumerate() {
            cancel.check()?;
            if !is_comment(token.kind()) {
                continue;
            }
            let previous = index.checked_sub(1).map(|index| &tokens[index]);
            let next = tokens.get(index + 1);
            let start = usize::from(token.text_range().start());
            let same_line = previous.is_some_and(|previous| {
                !source[usize::from(previous.text_range().end())..start].contains(['\r', '\n'])
            });
            let after_open = previous.is_some_and(|token| {
                matches!(
                    token.kind(),
                    SyntaxKind::LBrace | SyntaxKind::LParen | SyntaxKind::LBracket
                )
            });
            let before_close = next.is_none_or(|token| {
                matches!(
                    token.kind(),
                    SyntaxKind::RBrace | SyntaxKind::RParen | SyntaxKind::RBracket
                )
            });
            let attachment = if same_line && !after_open {
                Attachment::Trailing
            } else if after_open && before_close {
                Attachment::Dangling
            } else {
                Attachment::Leading
            };
            attachments.insert(start, attachment);
        }
        Ok(Self(attachments))
    }

    pub fn attachment(&self, token: &SyntaxToken) -> Option<Attachment> {
        self.0
            .get(&usize::from(token.text_range().start()))
            .copied()
    }
}

pub(crate) fn is_comment(kind: SyntaxKind) -> bool {
    matches!(kind, SyntaxKind::LineComment | SyntaxKind::BlockComment)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kagari_source::source::SourceFile;
    use kagari_syntax::{ast::traits::AstNode, parser::parse};

    #[test]
    fn attachment_uses_source_neighbors_and_preserves_nested_text() {
        let text = "// leading\nfn run() { /* dangling /* inner */ */ } // trailing";
        let parsed = parse(&SourceFile::new("comments.kgr", text));
        let root = parsed.syntax();
        let comments = Comments::collect(root.syntax(), text, &Default::default()).unwrap();
        let actual: Vec<_> = root
            .syntax()
            .descendants_with_tokens()
            .filter_map(|item| item.into_token())
            .filter_map(|token| comments.attachment(&token))
            .collect();
        assert_eq!(
            actual,
            [
                Attachment::Leading,
                Attachment::Dangling,
                Attachment::Trailing
            ]
        );
    }
}
