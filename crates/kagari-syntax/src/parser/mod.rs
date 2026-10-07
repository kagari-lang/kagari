//! Recoverable source parsing, strict syntax acceptance and parser resource limits.
//!
//! [`parse`] and its controlled variants return a tree even for malformed source;
//! inspect [`Parse::diagnostics`] before compiling it. [`parse_module`] instead
//! rejects any syntax diagnostics. [`parse_declarations`] enables offline interface
//! forms such as bodyless functions; it does not authorize execution.
//! Parsing builds Rowan nodes directly, without an intermediate event stream.

mod core;
mod grammar;

use rowan::GreenNode;
use {
    kagari_common::cancellation::{CancellationToken, Cancelled},
    kagari_source::source::SourceFile,
};

use crate::{
    BoxedDiagnosticBuffer, DiagnosticBuffer,
    ast::{item::SourceFile as AstSourceFile, traits::AstNode},
    lexer::lex_with_cancellation,
    parser::core::Parser,
    syntax_node::syntax_node_from_green,
};

/// Per-file parser resource limits. The limit diagnostic is additional to the
/// ordinary diagnostic budget; zero still permits parsing valid source.
/// Defaults are 256 diagnostics, 64 recursive grammar entries and tree depth 128.
/// These are parser limits, not limits on source bytes or lexer token allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseLimits {
    /// Maximum ordinary diagnostics before recording exhaustion and stopping grammar work.
    pub max_diagnostics: usize,
    /// Maximum simultaneously active recursive grammar entries.
    pub max_nesting: usize,
    /// Maximum completed CST node depth (tokens do not count).
    /// Checked when closing nodes; recovered output can exceed this threshold.
    pub max_tree_depth: usize,
}

impl Default for ParseLimits {
    fn default() -> Self {
        Self {
            max_diagnostics: 256,
            max_nesting: 64,
            max_tree_depth: 128,
        }
    }
}

/// An owned lossless green tree plus syntax diagnostics for one parse attempt.
///
/// The tree owns token text independently of the input source. AST handles from
/// [`Self::syntax`] retain it even after this result is dropped. File identity,
/// revision and parse mode are not stored here; callers keep that association.
/// A result with diagnostics is useful for tooling, not evidence of valid code.
#[derive(Debug, Clone)]
pub struct Parse {
    /// Immutable root storage, shared with cloned parse results and AST handles.
    green: GreenNode,
    /// Ordinary syntax errors and any parser-limit diagnostic, in emission order.
    diagnostics: DiagnosticBuffer,
}

impl Parse {
    /// Creates a typed source-root handle over the retained tree without reparsing.
    pub fn syntax(&self) -> AstSourceFile {
        AstSourceFile::cast(syntax_node_from_green(self.green.clone()))
            .expect("parser must always produce a source file node")
    }

    /// Borrows diagnostics in emission order; an empty list means no syntax errors were recorded.
    ///
    /// An empty list does not establish name resolution or type correctness.
    pub fn diagnostics(&self) -> &DiagnosticBuffer {
        &self.diagnostics
    }
}

/// Parses ordinary source with default limits, preserving recoverable syntax errors.
///
/// See the [crate example](crate) for a complete source and AST walk. Even a missing
/// operand leaves usable structure, as this example demonstrates:
///
/// # Examples
///
/// ```
/// use kagari_source::source::SourceFile;
/// use kagari_syntax::{ast::{expr::Expr, item::Item}, parser::parse};
/// let parsed = parse(&SourceFile::new("partial.kgr", "fn total() { total + }"));
/// assert!(!parsed.diagnostics().is_empty());
/// let Some(Item::FnDef(function)) = parsed.syntax().items().next() else {
///     panic!("the function is retained");
/// };
/// let Some(Expr::BinaryExpr(binary)) = function.body().and_then(|body| body.tail_expr()) else {
///     panic!("the incomplete binary expression is retained");
/// };
/// assert!(binary.lhs().is_some());
/// assert!(binary.rhs().is_none());
/// ```
pub fn parse(source: &SourceFile) -> Parse {
    parse_with_cancellation(source, &CancellationToken::default())
        .expect("fresh cancellation token")
}

/// Parses ordinary source using default limits and the caller's cancellation token.
///
/// # Errors
///
/// Returns [`Cancelled`] if cancellation is observed; syntax errors instead appear
/// in the successful [`Parse`]'s diagnostics. No partial result is published on cancellation.
pub fn parse_with_cancellation(
    source: &SourceFile,
    cancel: &CancellationToken,
) -> Result<Parse, Cancelled> {
    parse_with_limits(source, ParseLimits::default(), cancel)
}

/// Parses ordinary source using explicit parser limits and cooperative cancellation.
///
/// Exhaustion stops grammar work and retains remaining text in an error node with
/// a limit diagnostic. Limits do not prevent the lexer from materializing all tokens.
///
/// # Errors
///
/// Returns [`Cancelled`] on observed cancellation. Syntax errors and limit
/// exhaustion are reported inside [`Parse`], rather than as this function's error.
pub fn parse_with_limits(
    source: &SourceFile,
    limits: ParseLimits,
    cancel: &CancellationToken,
) -> Result<Parse, Cancelled> {
    parse_with_mode(source, limits, cancel, false)
}

/// Parse an offline interface. This does not authorize executable code generation.
///
/// Uses the same lossless tree and recovery as [`parse_with_limits`], while accepting
/// declaration-only forms such as `pub fn size() -> i32;` and top-level `type` items.
/// The caller must retain which mode produced the tree; [`Parse`] does not store it.
///
/// # Errors
///
/// Returns [`Cancelled`] on observed cancellation. Syntax and resource-limit errors
/// remain diagnostics in the returned tree.
pub fn parse_declarations(
    source: &SourceFile,
    limits: ParseLimits,
    cancel: &CancellationToken,
) -> Result<Parse, Cancelled> {
    parse_with_mode(source, limits, cancel, true)
}

/// Owns the lex/build/final-poll handoff; only a non-cancelled attempt publishes a tree.
fn parse_with_mode(
    source: &SourceFile,
    limits: ParseLimits,
    cancel: &CancellationToken,
    declarations: bool,
) -> Result<Parse, Cancelled> {
    let tokens = lex_with_cancellation(source.text(), cancel)?;
    let mut parser = Parser::new(source.text(), tokens, limits, cancel.clone());
    parser.declarations = declarations;
    parser.parse_root();
    let (green, diagnostics) = parser.finish();
    cancel.check()?;
    Ok(Parse { green, diagnostics })
}

/// Parses with default limits and returns an AST only when syntax diagnostics are empty.
///
/// This does not load modules or check names/types. Use [`parse`] when tooling
/// needs to retain the tree alongside diagnostics; this wrapper discards that tree.
///
/// # Errors
///
/// Returns all recorded diagnostics for malformed or limit-exhausted source.
pub fn parse_module(source: &SourceFile) -> Result<AstSourceFile, BoxedDiagnosticBuffer> {
    let parse = parse(source);
    if !parse.diagnostics.is_empty() {
        return Err(Box::new(parse.diagnostics));
    }
    Ok(parse.syntax())
}
