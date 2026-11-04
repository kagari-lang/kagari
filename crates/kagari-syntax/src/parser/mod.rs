//! Recoverable typed parsing with retained source identity and explicit parse mode.

mod core;
mod grammar;

use std::sync::Arc;

use kagari_common::{
    cancellation::{CancellationToken, Cancelled},
    span::Span,
};
use kagari_source::source::SourceFile;

use crate::{
    BoxedDiagnosticBuffer, DiagnosticBuffer,
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

/// Grammar-recursion and completed-chain limits, independent of allocation layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseLimits {
    pub max_diagnostics: usize,
    pub max_nesting: usize,
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
    Source,
    Declarations,
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
#[derive(Debug, Clone)]
pub struct Parse {
    pub(crate) file: Arc<ParsedFile>,
}

impl Parse {
    pub fn module(&self) -> ModuleSyntax {
        ModuleSyntax {
            file: self.file.clone(),
            id: self.file.root,
        }
    }

    pub fn source(&self) -> &SourceFile {
        &self.file.source
    }

    pub fn mode(&self) -> ParseMode {
        self.file.mode
    }

    pub fn shared_diagnostics(&self) -> Arc<DiagnosticBuffer> {
        self.file.diagnostics.clone()
    }

    pub fn diagnostics(&self) -> &DiagnosticBuffer {
        &self.file.diagnostics
    }

    /// The complete physical lexer sequence, including trivia, unknown input and EOF.
    pub fn tokens(&self) -> impl ExactSizeIterator<Item = LexicalToken<'_>> {
        (0..self.file.lexical.len()).map(|index| LexicalToken {
            owner: &self.file,
            index: index as u32,
        })
    }
}

pub fn parse(source: &SourceFile) -> Parse {
    parse_with_cancellation(source, &CancellationToken::default())
        .expect("fresh cancellation token")
}

pub fn parse_with_cancellation(
    source: &SourceFile,
    cancel: &CancellationToken,
) -> Result<Parse, Cancelled> {
    parse_with_limits(source, ParseLimits::default(), cancel)
}

pub fn parse_with_limits(
    source: &SourceFile,
    limits: ParseLimits,
    cancel: &CancellationToken,
) -> Result<Parse, Cancelled> {
    parse_with_mode(source, limits, cancel, ParseMode::Source)
}

pub fn parse_declarations(
    source: &SourceFile,
    limits: ParseLimits,
    cancel: &CancellationToken,
) -> Result<Parse, Cancelled> {
    parse_with_mode(source, limits, cancel, ParseMode::Declarations)
}

fn parse_with_mode(
    source: &SourceFile,
    limits: ParseLimits,
    cancel: &CancellationToken,
    mode: ParseMode,
) -> Result<Parse, Cancelled> {
    let tokens = lex_with_cancellation(source.text(), cancel)?;
    let lexical = Lexical::new(source.text(), tokens, cancel)?;
    cancel.check()?;
    let mut parser = Parser::new(source, lexical, limits, cancel.clone(), mode);
    let root = parser.parse_root();
    let file = parser.finish(root);
    cancel.check()?;
    Ok(Parse {
        file: Arc::new(file),
    })
}

pub fn parse_module(source: &SourceFile) -> Result<ModuleSyntax, BoxedDiagnosticBuffer> {
    let parsed = parse(source);
    if !parsed.diagnostics().is_empty() {
        return Err(Box::new(parsed.diagnostics().clone()));
    }
    Ok(parsed.module())
}
