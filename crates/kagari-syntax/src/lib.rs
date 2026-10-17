//! Lossless parsing and typed syntax views for one Kagari source file.
//!
//! [`lexer`] produces token kinds and byte ranges. [`parser`] builds a concrete
//! syntax tree (CST) retaining punctuation, whitespace, comments and erroneous
//! text. [`ast`] supplies typed views over that same tree. Name resolution and
//! type checking belong to the subsequent HIR analysis, not to this crate.
//!
//! Start with [`parser::parse`] for recoverable syntax or [`parser::parse_module`]
//! to reject syntax diagnostics. Controlled entrypoints accept cancellation and
//! [`parser::ParseLimits`]. Callers retain the source identity/revision association.
//!
//! # Examples
//!
//! ```
//! use kagari_source::source::SourceFile;
//! use kagari_syntax::{ast::item::Item, parser::parse};
//!
//! let source = SourceFile::new("sum.kgr", "fn total() -> i32 { 1 + 2 }");
//! let parsed = parse(&source);
//! assert!(parsed.diagnostics().is_empty());
//! let Some(Item::FnDef(function)) = parsed.syntax().items().next() else {
//!     panic!("the example contains one function");
//! };
//! assert_eq!(function.name_text().as_deref(), Some("total"));
//! assert!(function.body().and_then(|body| body.tail_expr()).is_some());
//! ```

use kagari_source::diagnostic::Diagnostic;
use smallvec::SmallVec;

pub mod ast;
pub mod kind;
pub mod lexer;
pub mod parser;
pub mod syntax_node;
pub mod token;

/// Ordered lexer output; 64 tokens fit inline, with heap growth beyond that.
pub type TokenBuffer = SmallVec<[token::Token; 64]>;
/// Syntax diagnostics; four fit inline. Capacity is not a diagnostic limit.
pub type DiagnosticBuffer = SmallVec<[Diagnostic; 4]>;
/// Owned diagnostics returned when [`parser::parse_module`] rejects a tree.
pub type BoxedDiagnosticBuffer = Box<DiagnosticBuffer>;

#[cfg(test)]
mod tests;
