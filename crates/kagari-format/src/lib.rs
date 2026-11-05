//! Pure lossless typed-AST formatting with separate syntax, comment and layout owners.
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
    ast::visit::{ElementRef, NodeRef},
    kind::SyntaxKind,
    lexer::lex_with_cancellation,
    parser::{ParseOptions, parse},
    token::TokenKind,
};

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

/// Formats one captured source, verifying reparse, token and typed grammar preservation.
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
    let parsed = parse(source, ParseOptions::default(), cancel)?;
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
    let module = parsed.module();
    let root = NodeRef::ModuleBody(module.body());
    let attached = Comments::collect(root, source.text(), cancel)?;
    let doc = syntax::document(root, &attached, cancel)?;
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
    let reparsed = parse(
        &SourceFile::new(source.name(), &output),
        ParseOptions::default(),
        cancel,
    )?;
    if !reparsed.diagnostics().is_empty()
        || shape(root, cancel)? != shape(NodeRef::ModuleBody(reparsed.module().body()), cancel)?
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
    root: NodeRef<'_>,
    cancel: &CancellationToken,
) -> Result<Vec<(bool, SyntaxKind, String)>, Cancelled> {
    fn visit(
        node: NodeRef<'_>,
        cancel: &CancellationToken,
        shape: &mut Vec<(bool, SyntaxKind, String)>,
    ) -> Result<(), Cancelled> {
        cancel.check()?;
        shape.push((true, node.kind(), String::new()));
        for element in node.elements() {
            match element {
                ElementRef::Node(child) => visit(child, cancel, shape)?,
                ElementRef::Token(token) if token.kind() != SyntaxKind::Whitespace => {
                    shape.push((true, token.kind(), token.text().into()))
                }
                ElementRef::Operator(operator) => {
                    shape.push((true, operator.kind(), operator.text().into()))
                }
                ElementRef::Missing { .. } => unreachable!("formatter rejects malformed source"),
                _ => {}
            }
        }
        shape.push((false, node.kind(), String::new()));
        Ok(())
    }
    let mut shape = Vec::new();
    visit(root, cancel, &mut shape)?;
    Ok(shape)
}
