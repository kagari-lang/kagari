//! Pure lossless-CST formatting with separate syntax, comment and layout owners.
//! No filesystem operations or semantic analysis occur in this crate.

mod comments;
mod layout;
mod syntax;
#[cfg(test)]
mod tests;

use crate::comments::Comments;
use kagari_common::cancellation::{CancellationToken, Cancelled};
use kagari_source::{presentation::DiagnosticRecord, source::SourceFile};
use kagari_syntax::{
    ast::traits::AstNode, kind::SyntaxKind, lexer::lex_with_cancellation,
    parser::parse_with_cancellation, syntax_node::SyntaxNode, token::TokenKind,
};
use rowan::WalkEvent;

/// Layout choices; width is a target and never splits literal or comment text.
#[derive(Debug, Clone, Copy)]
pub struct Options {
    pub indent_width: usize,
    pub max_width: usize,
    pub newline: Newline,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            indent_width: 4,
            max_width: 100,
            newline: Newline::Preserve,
        }
    }
}

/// Line endings inserted for layout; literal and comment spelling is preserved.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Newline {
    #[default]
    Preserve,
    Lf,
    Crlf,
}

/// Failure leaves the caller's input untouched; no partial formatted text is returned.
#[derive(Debug)]
pub enum FormatError {
    Diagnostics(Vec<DiagnosticRecord>),
    Cancelled,
    InvalidOptions,
    Preservation,
}

impl From<Cancelled> for FormatError {
    fn from(_: Cancelled) -> Self {
        Self::Cancelled
    }
}

/// Formats one captured source, verifying reparse, token and CST preservation.
/// Syntax errors retain original revision-bound diagnostics. Semantic errors are
/// irrelevant to formatting. Cancellation and failed preservation return no text.
pub fn format(
    source: &SourceFile,
    options: Options,
    cancel: &CancellationToken,
) -> Result<String, FormatError> {
    if !(1..=16).contains(&options.indent_width) || !(20..=1000).contains(&options.max_width) {
        return Err(FormatError::InvalidOptions);
    }
    let parsed = parse_with_cancellation(source, cancel)?;
    if !parsed.diagnostics().is_empty() {
        return Err(FormatError::Diagnostics(
            parsed
                .diagnostics()
                .iter()
                .cloned()
                .map(|diagnostic| DiagnosticRecord::from_diagnostic(diagnostic, source))
                .collect(),
        ));
    }
    let root = parsed.syntax();
    let attached = Comments::collect(root.syntax(), source.text(), cancel)?;
    let doc = syntax::document(root.syntax(), &attached, cancel)?;
    let newline = match options.newline {
        Newline::Lf => "\n",
        Newline::Crlf => "\r\n",
        Newline::Preserve => {
            let text = source.text();
            if text.contains('\n')
                && text.bytes().enumerate().all(|(index, ch)| {
                    ch != b'\n' || index > 0 && text.as_bytes()[index - 1] == b'\r'
                })
            {
                "\r\n"
            } else {
                "\n"
            }
        }
    };
    let output = layout::print(
        &doc,
        options.max_width,
        options.indent_width,
        newline,
        cancel,
    )?;
    let reparsed = parse_with_cancellation(&SourceFile::new(source.name(), &output), cancel)?;
    if !reparsed.diagnostics().is_empty()
        || shape(root.syntax(), cancel)? != shape(reparsed.syntax().syntax(), cancel)?
        || tokens(source.text(), cancel)? != tokens(&output, cancel)?
    {
        return Err(FormatError::Preservation);
    }
    Ok(output)
}

fn tokens(text: &str, cancel: &CancellationToken) -> Result<Vec<(TokenKind, String)>, Cancelled> {
    Ok(lex_with_cancellation(text, cancel)?
        .iter()
        .filter(|token| token.kind != TokenKind::Whitespace)
        .map(|token| {
            (
                token.kind.clone(),
                text[token.span.start..token.span.end].into(),
            )
        })
        .collect())
}

fn shape(
    root: &SyntaxNode,
    cancel: &CancellationToken,
) -> Result<Vec<(bool, SyntaxKind, String)>, Cancelled> {
    let mut shape = Vec::new();
    for event in root.preorder_with_tokens() {
        cancel.check()?;
        match event {
            WalkEvent::Enter(element) if element.kind() != SyntaxKind::Whitespace => shape.push((
                true,
                element.kind(),
                element
                    .as_token()
                    .map_or(String::new(), |token| token.text().into()),
            )),
            WalkEvent::Leave(element) if element.as_node().is_some() => {
                shape.push((false, element.kind(), String::new()))
            }
            _ => {}
        }
    }
    Ok(shape)
}
