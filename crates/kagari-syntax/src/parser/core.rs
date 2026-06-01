use kagari_common::{Diagnostic, DiagnosticKind};
use rowan::{Checkpoint, GreenNode, GreenNodeBuilder, Language};
use smallvec::SmallVec;

use crate::{
    DiagnosticBuffer, TokenBuffer,
    kind::SyntaxKind,
    syntax_node::KagariLanguage,
    token::{Token, TokenKind},
};

pub(crate) struct Parser<'a> {
    text: &'a str,
    tokens: TokenBuffer,
    cursor: usize,
    builder: GreenNodeBuilder<'static>,
    diagnostics: DiagnosticBuffer,
    allow_struct_literals: bool,
}

impl<'a> Parser<'a> {
    pub(crate) fn new(text: &'a str, tokens: TokenBuffer) -> Self {
        Self {
            text,
            tokens,
            cursor: 0,
            builder: GreenNodeBuilder::new(),
            diagnostics: SmallVec::new(),
            allow_struct_literals: true,
        }
    }

    pub(crate) fn finish(self) -> (GreenNode, DiagnosticBuffer) {
        (self.builder.finish(), self.diagnostics)
    }

    pub(crate) fn start_node(&mut self, kind: SyntaxKind) {
        self.builder.start_node(KagariLanguage::kind_to_raw(kind));
    }

    pub(crate) fn finish_node(&mut self) {
        self.builder.finish_node();
    }

    pub(crate) fn checkpoint(&mut self) -> Checkpoint {
        self.builder.checkpoint()
    }

    pub(crate) fn start_node_at(&mut self, checkpoint: Checkpoint, kind: SyntaxKind) {
        self.builder
            .start_node_at(checkpoint, KagariLanguage::kind_to_raw(kind));
    }

    pub(crate) fn expect(&mut self, kind: TokenKind, diagnostic: DiagnosticKind) -> bool {
        self.bump_trivia();
        if self.at(kind) {
            self.bump();
            true
        } else {
            self.error_here(diagnostic);
            false
        }
    }

    pub(crate) fn bump_trivia(&mut self) {
        while self
            .peek()
            .map(|token| token.kind.is_trivia())
            .unwrap_or(false)
        {
            self.bump();
        }
    }

    pub(crate) fn bump_as_error(&mut self) {
        self.start_node(SyntaxKind::Error);
        self.bump();
        self.finish_node();
    }

    pub(crate) fn bump(&mut self) {
        if let Some(token) = self.peek().cloned() {
            let kind = token.kind.to_syntax_kind();
            let text = &self.text[token.span.start..token.span.end];
            self.builder.token(KagariLanguage::kind_to_raw(kind), text);
            self.cursor += 1;
        }
    }

    pub(crate) fn error_here(&mut self, kind: DiagnosticKind) {
        let span = self.peek().map(|token| token.span).unwrap_or_default();
        self.diagnostics
            .push(Diagnostic::error(kind).with_span(span));
    }

    pub(crate) fn push_diagnostic(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    pub(crate) fn at(&self, kind: TokenKind) -> bool {
        self.current_kind() == Some(kind)
    }

    pub(crate) fn at_any(&self, kinds: &[TokenKind]) -> bool {
        self.current_kind()
            .map(|kind| kinds.contains(&kind))
            .unwrap_or(false)
    }

    pub(crate) fn current_kind(&self) -> Option<TokenKind> {
        self.peek().map(|token| token.kind.clone())
    }

    pub(crate) fn allow_struct_literals(&self) -> bool {
        self.allow_struct_literals
    }

    pub(crate) fn with_struct_literals_allowed<T>(
        &mut self,
        allowed: bool,
        f: impl FnOnce(&mut Self) -> T,
    ) -> T {
        let previous = self.allow_struct_literals;
        self.allow_struct_literals = allowed;
        let result = f(self);
        self.allow_struct_literals = previous;
        result
    }

    pub(crate) fn nth_nontrivia_kind(&self, n: usize) -> Option<TokenKind> {
        self.tokens
            .iter()
            .skip(self.cursor)
            .filter(|token| !token.kind.is_trivia())
            .nth(n)
            .map(|token| token.kind.clone())
    }

    pub(crate) fn cursor(&self) -> usize {
        self.cursor
    }

    pub(crate) fn nth_nontrivia_kind_from(&self, cursor: &mut usize) -> Option<TokenKind> {
        while let Some(token) = self.tokens.get(*cursor) {
            *cursor += 1;
            if !token.kind.is_trivia() {
                return Some(token.kind.clone());
            }
        }
        None
    }

    pub(crate) fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.cursor)
    }
}
