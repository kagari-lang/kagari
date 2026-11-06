//! Lossless parsing and typed syntax views for one Kagari source file.
//!
//! [`lexer`] produces token kinds and byte ranges. [`parser`] directly constructs
//! the owned grammar fields in [`ast`], retaining punctuation, missing roles and
//! skipped input beside a shared source-backed lexical table. Name resolution and
//! type checking belong to the subsequent HIR analysis, not to this crate.
//!
//! [`parser::parse`] accepts [`parser::ParseOptions`] and a cancellation token.
//! Its result retains recoverable syntax and diagnostics; callers decide whether
//! diagnostics prevent their operation. Callers retain the source identity/revision
//! association.
//!
//! # Examples
//!
//! ```
//! use kagari_common::cancellation::CancellationToken;
//! use kagari_source::source::SourceFile;
//! use kagari_syntax::{ast::views::ItemView, parser::{ParseOptions, parse}};
//!
//! let source = SourceFile::new("sum.kgr", "fn total() -> i32 { 1 + 2 }");
//! let parsed = parse(&source, ParseOptions::default(), &CancellationToken::default())?;
//! assert!(parsed.diagnostics().is_empty());
//! let module = parsed.module();
//! let Some(ItemView::FnDef(function)) = module.items().next() else {
//!     panic!("the example contains one function");
//! };
//! assert_eq!(function.name_text().as_deref(), Some("total"));
//! assert!(function.body().and_then(|body| body.tail_expr()).is_some());
//! # Ok::<(), kagari_common::cancellation::Cancelled>(())
//! ```

use kagari_source::diagnostic::Diagnostic;
use smallvec::SmallVec;

pub mod ast;
pub mod kind;
pub mod lexer;
mod lexical;
pub mod parser;
pub mod token;

/// Ordered lexer output; 64 tokens fit inline, with heap growth beyond that.
pub type TokenBuffer = SmallVec<[token::Token; 64]>;
/// Syntax diagnostics; four fit inline. Capacity is not a diagnostic limit.
pub type DiagnosticBuffer = SmallVec<[Diagnostic; 4]>;

#[cfg(test)]
mod tests;
