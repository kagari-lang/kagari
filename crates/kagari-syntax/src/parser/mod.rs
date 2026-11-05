//! Recoverable typed parsing with retained source identity and explicit parse mode.

mod core;
mod grammar;

use std::sync::Arc;

#[cfg(test)]
use crate::tests::cancellation::{Checkpoint, checkpoint};

use kagari_common::{
    cancellation::{CancellationToken, Cancelled},
    span::Span,
};
use kagari_source::source::SourceFile;

use crate::{
    DiagnosticBuffer,
    ast::visit::LexicalToken,
    ast::{
        item::{ModuleBody, ModuleId},
        module::ModuleSyntax,
        site::{Recovery, SyntaxSite},
    },
    lexer::lex_with_cancellation,
    lexical::Lexical,
    parser::core::Parser,
};

/// Diagnostic and grammar-growth limits for one physical source file.
///
/// Inline modules share the file's limits; these are not per-function budgets.
/// Exhaustion stops further grammar growth, adds a `CompileLimitExceeded`
/// diagnostic and retains the unparsed source suffix. It does not turn [`parse`]
/// into an error result: only cancellation returns [`Cancelled`].
///
/// These limits do not bound source bytes, token count, total memory or elapsed
/// time. Raising the depth limits weakens protection against stack exhaustion
/// during parsing and recursive destruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseLimits {
    /// Maximum ordinary syntax diagnostics retained before stopping recovery.
    /// Defaults to 256.
    ///
    /// The limit diagnostic is additional: a budget of N allows up to N ordinary
    /// diagnostics plus one limit diagnostic. Zero still accepts valid syntax;
    /// the first attempted syntax diagnostic instead stops parsing with the
    /// limit diagnostic. This does not limit subsequent HIR/type-check diagnostics.
    pub max_diagnostics: usize,

    /// Maximum simultaneously active guarded recursive grammar entries.
    /// Defaults to 64.
    ///
    /// Nested groups such as `(((value)))`, nested types and nested modules consume
    /// this budget. Each entry releases its slot on return, so sibling statements
    /// do not accumulate nesting. The count follows parser grammar entries, not
    /// literal parentheses, indentation or every Rust stack frame. Zero prevents
    /// entering the first guarded recursive grammar call.
    pub max_nesting: usize,

    /// Maximum syntax-node depth admitted while constructing the AST.
    /// Defaults to 128.
    ///
    /// A leaf has depth one; a parent adds one to its deepest child. Grammar
    /// wrappers, names and signatures count; tokens and trivia do not. Sibling
    /// count does not add depth. For example, `a + b + c` builds nested binary
    /// nodes even when a parser loop constructs them without equally deep
    /// recursion, so this limit also covers chains that `max_nesting` cannot bound.
    ///
    /// Zero rejects even an empty file's module node. This is a growth limit,
    /// not a strict depth cap on the recovered result: already active enclosing
    /// grammar scopes still close and can add outer wrappers after stopping.
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

/// Declaration-mode provenance is retained and cannot imply executable acceptance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseMode {
    /// Ordinary script source, with normal function-body and type-alias requirements.
    Source,
    /// Admits bodyless native function signatures and opaque type declarations.
    /// This does not validate those declarations against an installed host contract.
    Declarations,
}

/// Grammar mode and resource limits for one physical source file.
///
/// Defaults select ordinary source grammar and [`ParseLimits::default`].
/// Cancellation belongs to the individual [`parse`] operation, not these options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseOptions {
    /// Grammar policy, retained in [`Parse::mode`]. Defaults to [`ParseMode::Source`].
    pub mode: ParseMode,
    /// Per-physical-file limits shared by all inline modules in this operation.
    pub limits: ParseLimits,
}

impl Default for ParseOptions {
    fn default() -> Self {
        Self {
            mode: ParseMode::Source,
            limits: ParseLimits::default(),
        }
    }
}

#[derive(Debug)]
pub(crate) struct ParsedFile {
    pub source: SourceFile,
    pub mode: ParseMode,
    pub lexical: Lexical,
    pub modules: Vec<ModuleBody>,
    pub recoveries: Vec<Recovery>,
    pub diagnostics: Arc<DiagnosticBuffer>,
    pub root: ModuleId,
}

impl ParsedFile {
    pub(crate) fn body_span(&self, site: SyntaxSite) -> Span {
        self.lexical.body_span(site.resolve(&self.recoveries))
    }

    pub(crate) fn full_span(&self, site: SyntaxSite) -> Span {
        self.lexical.full_span(site.resolve(&self.recoveries))
    }
}

/// One immutable physical parse. Clones share nodes, lexical records and source backing.
///
/// A completed parse can contain syntax errors or a resource-limit diagnostic.
/// Inspect [`Self::diagnostics`] before requiring valid syntax. Cloning preserves
/// parse-local identity; calling [`parse`] again creates a different identity even
/// for unchanged text. Keep a Parse or [`ModuleSyntax`] to retain borrowed AST views.
#[derive(Debug, Clone)]
pub struct Parse {
    pub(crate) file: Arc<ParsedFile>,
}

impl Parse {
    /// Retain the physical file's root module, including any recovered syntax.
    /// The returned handle can outlive this Parse and shares its backing allocation.
    pub fn module(&self) -> ModuleSyntax {
        ModuleSyntax {
            file: self.file.clone(),
            id: self.file.root,
        }
    }

    /// The captured source snapshot used to produce every token and diagnostic span.
    pub fn source(&self) -> &SourceFile {
        &self.file.source
    }

    /// The grammar mode used for this snapshot, not a statement of semantic validity.
    pub fn mode(&self) -> ParseMode {
        self.file.mode
    }

    /// Share the immutable physical-file diagnostics without copying individual records.
    /// This handle does not retain the AST or source text; retain the source separately
    /// when presenting the diagnostic spans after dropping the parse.
    pub fn shared_diagnostics(&self) -> Arc<DiagnosticBuffer> {
        self.file.diagnostics.clone()
    }

    /// Syntax and resource-limit diagnostics for the entire physical file.
    /// Includes inline modules; name resolution and type-check diagnostics belong to HIR.
    pub fn diagnostics(&self) -> &DiagnosticBuffer {
        &self.file.diagnostics
    }

    /// The complete physical lexer sequence, including trivia, unknown input and EOF.
    /// Missing grammar roles never fabricate tokens. Original spellings concatenate
    /// to the complete source even when grammar recovery stopped before its end.
    /// This iterator reads the lexical table directly without collecting AST children.
    pub fn tokens(&self) -> impl ExactSizeIterator<Item = LexicalToken<'_>> {
        (0..self.file.lexical.len()).map(|index| LexicalToken {
            owner: &self.file,
            index: index as u32,
        })
    }
}

/// Parse one physical source into a retained, recoverable syntax snapshot.
///
/// Syntax errors and resource exhaustion remain in [`Parse::diagnostics`]; callers
/// decide whether those diagnostics prevent their operation. The snapshot retains
/// the source, selected mode, missing grammar roles and unparsed recovery suffix.
/// Declaration mode admits native declarations but does not establish executable
/// or installed-contract validity.
///
/// # Errors
///
/// Returns [`Cancelled`] when cancellation is observed. No partial parse is returned.
///
/// # Panics
///
/// The compact lexical representation requires source byte length and lexical
/// record count to fit in `u32`. Exceeding those representation bounds panics;
/// [`ParseLimits`] is not a source-size admission policy.
///
/// # Examples
///
/// ```
/// use kagari_common::cancellation::CancellationToken;
/// use kagari_source::source::SourceFile;
/// use kagari_syntax::parser::{ParseOptions, parse};
///
/// let source = SourceFile::new("example.kgr", "fn answer() -> i32 { 42 }");
/// let cancel = CancellationToken::default();
/// let parsed = parse(&source, ParseOptions::default(), &cancel)?;
/// assert!(parsed.diagnostics().is_empty());
/// assert_eq!(parsed.module().text(), source.text());
/// # Ok::<(), kagari_common::cancellation::Cancelled>(())
/// ```
pub fn parse(
    source: &SourceFile,
    options: ParseOptions,
    cancel: &CancellationToken,
) -> Result<Parse, Cancelled> {
    let tokens = lex_with_cancellation(source.text(), cancel)?;
    let lexical = Lexical::new(source.text(), tokens, cancel)?;
    cancel.check()?;
    let mut parser = Parser::new(
        source,
        lexical,
        options.limits,
        cancel.clone(),
        options.mode,
    );
    let root = parser.parse_root();
    let file = parser.finish(root);
    #[cfg(test)]
    checkpoint(Checkpoint::Publish, file.lexical.len());
    cancel.check()?;
    Ok(Parse {
        file: Arc::new(file),
    })
}
