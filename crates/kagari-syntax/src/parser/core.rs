//! Cursor, diagnostics, sparse recovery ownership and admission before recursive growth.

use std::sync::Arc;

#[cfg(test)]
use crate::tests::cancellation::{Checkpoint, checkpoint};

use kagari_common::cancellation::CancellationToken;
use kagari_source::{
    diagnostic::{Diagnostic, DiagnosticKind},
    literal::decode_string_literal_with_cancellation,
    source::SourceFile,
};

use crate::{
    DiagnosticBuffer,
    ast::{
        borrow::NodeData,
        item::{ModuleBody, ModuleId},
        site::{
            ListEntry, Recovery, RecoveryPart, Required, SeparatedList, SyntaxSite, Unexpected,
        },
        tokens::{OperatorToken, Token},
    },
    kind::SyntaxKind,
    lexical::Lexical,
    parser::{ParseLimits, ParseMode, ParsedFile},
    token::TokenKind,
};

const CANCELLATION_CHECK_TOKENS: usize = 1024;

pub(crate) struct Built<T> {
    pub node: Required<T>,
    pub depth: usize,
}

impl<T> Built<T> {
    pub fn missing(at: u32) -> Self {
        Self {
            node: Required::Missing { at },
            depth: 0,
        }
    }

    pub fn present(node: T, depth: usize) -> Self {
        Self {
            node: Required::Present(node),
            depth,
        }
    }
}

pub(crate) struct Mark {
    first: u32,
    frame: usize,
}

struct RecoveryFrame {
    field: u16,
    entry: u32,
    part: RecoveryPart,
    unexpected: Vec<Unexpected>,
    deepest_child: usize,
}

pub(crate) struct Parser<'a> {
    source: &'a SourceFile,
    pub(crate) lexical: Lexical,
    cursor: usize,
    diagnostics: DiagnosticBuffer,
    pub(crate) modules: Vec<Option<ModuleBody>>,
    recoveries: Vec<Recovery>,
    frames: Vec<RecoveryFrame>,
    limits: ParseLimits,
    nesting: usize,
    stopped: Option<usize>,
    pub(crate) completed_depth: usize,
    pub(crate) cancel: CancellationToken,
    pub(crate) mode: ParseMode,
    pub(crate) allow_struct_literals: bool,
}

impl<'a> Parser<'a> {
    pub(crate) fn new(
        source: &'a SourceFile,
        lexical: Lexical,
        limits: ParseLimits,
        cancel: CancellationToken,
        mode: ParseMode,
    ) -> Self {
        Self {
            source,
            lexical,
            cursor: 0,
            diagnostics: DiagnosticBuffer::new(),
            modules: Vec::new(),
            recoveries: Vec::new(),
            frames: Vec::new(),
            limits,
            nesting: 0,
            stopped: None,
            completed_depth: 0,
            cancel,
            mode,
            allow_struct_literals: true,
        }
    }

    pub(crate) fn finish(self, root: ModuleId) -> ParsedFile {
        assert!(self.frames.is_empty(), "closed grammar recovery scopes");
        ParsedFile {
            source: self.source.clone(),
            mode: self.mode,
            lexical: self.lexical,
            modules: self
                .modules
                .into_iter()
                .map(|body| body.expect("completed module body"))
                .collect(),
            recoveries: self.recoveries,
            diagnostics: Arc::new(self.diagnostics),
            root,
        }
    }

    pub(crate) fn begin(&mut self) -> Mark {
        self.bump_trivia();
        self.begin_at(self.cursor as u32, 0)
    }

    pub(crate) fn begin_at(&mut self, first: u32, child_depth: usize) -> Mark {
        let mark = Mark {
            first,
            frame: self.frames.len(),
        };
        self.frames.push(RecoveryFrame {
            field: 0,
            entry: 0,
            part: RecoveryPart::Element,
            unexpected: Vec::new(),
            deepest_child: child_depth,
        });
        mark
    }

    pub(crate) fn end(&mut self, mark: Mark) -> SyntaxSite {
        assert_eq!(mark.frame + 1, self.frames.len());
        let frame = self.frames.pop().expect("open grammar recovery scope");
        let depth = frame.deepest_child + 1;
        self.account_depth(depth);
        let mut end = self.cursor;
        while end > mark.first as usize && self.lexical.kind(end - 1).is_trivia() {
            end -= 1;
        }
        let site = SyntaxSite {
            first: mark.first,
            end: end as u32,
        };
        if frame.unexpected.is_empty() {
            site
        } else {
            self.recoveries.push(Recovery {
                site,
                unexpected: frame.unexpected,
            });
            SyntaxSite {
                first: self
                    .recoveries
                    .len()
                    .try_into()
                    .expect("parse-local recovery index"),
                end: 0,
            }
        }
    }

    pub(crate) fn first<T: NodeData>(&self, built: &Built<T>, fallback: usize) -> u32 {
        built.node.as_ref().map_or(fallback as u32, |node| {
            node.site().resolve(&self.recoveries).first
        })
    }

    pub(crate) fn account_depth(&mut self, depth: usize) {
        self.completed_depth = depth;
        if let Some(parent) = self.frames.last_mut() {
            parent.deepest_child = parent.deepest_child.max(depth);
        }
        if depth > self.limits.max_tree_depth {
            self.limit("syntax tree depth", self.limits.max_tree_depth, self.cursor);
        }
    }

    pub(crate) fn current_text(&self) -> &str {
        let span = self.lexical.span(self.cursor);
        &self.source.text()[span.start..span.end]
    }

    pub(crate) fn abandon(&mut self, mark: Mark, suffix: usize) {
        assert_eq!(mark.frame + 1, self.frames.len());
        self.frames.pop();
        self.stopped = Some(self.stopped.map_or(suffix, |first| first.min(suffix)));
    }

    pub(crate) fn position(&mut self, field: u16, entry: usize) {
        let frame = self.frames.last_mut().expect("owning grammar scope");
        frame.field = field;
        frame.entry = entry.try_into().expect("parse-local list index");
        frame.part = RecoveryPart::Element;
    }

    pub(crate) fn unexpected_range(&mut self, first: usize, end: usize) {
        if first >= end {
            return;
        }
        let frame = self.frames.last_mut().expect("unexpected input owner");
        if let Some(previous) = frame.unexpected.last_mut()
            && previous.field == frame.field
            && previous.entry == frame.entry
            && previous.part == frame.part
            && previous.end as usize == first
        {
            previous.end = end as u32;
        } else {
            frame.unexpected.push(Unexpected {
                field: frame.field,
                entry: frame.entry,
                part: frame.part,
                first: first as u32,
                end: end as u32,
            });
        }
    }

    pub(crate) fn bump_as_error(&mut self) {
        if self.at(TokenKind::Eof) {
            return;
        }
        let first = self.cursor;
        self.cursor += 1;
        self.unexpected_range(first, self.cursor);
        self.bump_trivia();
    }

    /// Parses one rejected grammatical entry for delimiter recovery, then retains
    /// only its physical coverage at the enclosing owner's insertion position.
    pub(crate) fn rejected<T>(&mut self, parse: impl FnOnce(&mut Self) -> T) {
        let first = self.cursor;
        let recoveries = self.recoveries.len();
        drop(parse(self));
        self.recoveries.truncate(recoveries);
        self.unexpected_range(first, self.cursor);
    }

    pub(crate) fn finish_remaining_tokens(&mut self) {
        if let Some(first) = self.stopped {
            self.position(u16::MAX, 0);
            self.unexpected_range(first, self.lexical.len());
            self.cursor = self.lexical.len();
        } else if self.at(TokenKind::Eof) {
            self.cursor = self.lexical.len();
        }
    }

    pub(crate) fn with_nesting<T>(&mut self, parse: impl FnOnce(&mut Self) -> T) -> Option<T> {
        if self.stopped.is_some() {
            return None;
        }
        if self.nesting >= self.limits.max_nesting {
            self.limit("parser nesting", self.limits.max_nesting, self.cursor);
            return None;
        }
        self.nesting += 1;
        let result = parse(self);
        self.nesting -= 1;
        Some(result)
    }

    /// Call before attaching a recursively owned parent, including iterative chains.
    pub(crate) fn admit(&mut self, depth: usize, suffix: usize) -> bool {
        if self.stopped.is_some() {
            return false;
        }
        if depth > self.limits.max_tree_depth {
            self.limit("syntax tree depth", self.limits.max_tree_depth, suffix);
            return false;
        }
        true
    }

    fn limit(&mut self, resource: &'static str, limit: usize, suffix: usize) {
        if self.stopped.is_none() {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::CompileLimitExceeded { resource, limit })
                    .with_span(self.lexical.span(self.cursor)),
            );
            self.stopped = Some(suffix);
        }
    }

    pub(crate) fn error_here(&mut self, mut kind: DiagnosticKind) {
        if self.stopped.is_some() {
            return;
        }
        let span = self.lexical.span(self.cursor);
        if self.at(TokenKind::Unknown) && !matches!(kind, DiagnosticKind::InvalidLiteral { .. }) {
            let text = &self.source.text()[span.start..span.end];
            let reason = if text == "f" && self.source.text()[span.end..].starts_with('"') {
                Some(
                    "the f String prefix is not supported; use ordinary quotes and name-only interpolation",
                )
            } else if text.starts_with('"') || text.starts_with("r\"") || text.starts_with("r#") {
                decode_string_literal_with_cancellation(text, &self.cancel)
                    .ok()
                    .and_then(Result::err)
            } else {
                None
            };
            if let Some(reason) = reason {
                kind = DiagnosticKind::InvalidLiteral {
                    reason: reason.into(),
                };
            }
        }
        if self.diagnostics.len() < self.limits.max_diagnostics {
            self.diagnostics
                .push(Diagnostic::error(kind).with_span(span));
        } else {
            self.limit(
                "parser diagnostics",
                self.limits.max_diagnostics,
                self.cursor,
            );
        }
    }

    pub(crate) fn expect<K>(
        &mut self,
        kind: TokenKind,
        diagnostic: DiagnosticKind,
    ) -> Required<Token<K>> {
        self.bump_trivia();
        if self.at(kind) && !self.at(TokenKind::Eof) {
            Required::Present(self.take())
        } else {
            let at = self.anchor();
            self.error_here(diagnostic);
            Required::Missing { at }
        }
    }

    pub(crate) fn take<K>(&mut self) -> Token<K> {
        self.bump_trivia();
        // Cancellation may arrive after the grammar admitted this token. Finish
        // that physical read; the entrypoint rejects the entire cancelled parse.
        assert_ne!(
            self.lexical.kind(self.cursor),
            TokenKind::Eof,
            "present grammatical token"
        );
        let token = Token::new(self.cursor as u32);
        self.cursor += 1;
        self.bump_trivia();
        token
    }

    pub(crate) fn eat<K>(&mut self, kind: TokenKind) -> Option<Token<K>> {
        self.bump_trivia();
        self.at(kind).then(|| self.take())
    }

    pub(crate) fn operator<O>(&mut self, operation: O, pieces: usize) -> OperatorToken<O> {
        let first = self.cursor;
        self.cursor += pieces;
        let end = self.cursor;
        self.bump_trivia();
        OperatorToken {
            operation,
            first: first as u32,
            end: end as u32,
        }
    }

    /// Observe cancellation at grammar sequence boundaries, preserving the first
    /// stopped suffix. Queries and depth accounting only consult this local stop.
    pub(crate) fn continue_parsing(&mut self) -> bool {
        if self.stopped.is_some() {
            return false;
        }
        let cancelled = self.cancel.check().is_err();
        #[cfg(test)]
        checkpoint(Checkpoint::Grammar, self.cursor);
        if cancelled {
            self.stopped = Some(self.cursor);
            return false;
        }
        true
    }

    pub(crate) fn bump_trivia(&mut self) {
        while self.lexical.kind(self.cursor).is_trivia() {
            if self.cursor.is_multiple_of(CANCELLATION_CHECK_TOKENS) && !self.continue_parsing() {
                break;
            }
            self.cursor += 1;
        }
    }

    pub(crate) fn at(&self, kind: TokenKind) -> bool {
        self.current_kind() == kind
    }

    pub(crate) fn at_any(&self, kinds: &[TokenKind]) -> bool {
        kinds.contains(&self.current_kind())
    }

    pub(crate) fn current_kind(&self) -> TokenKind {
        if self.stopped.is_some() {
            TokenKind::Eof
        } else {
            self.lexical.kind(self.cursor)
        }
    }

    pub(crate) fn anchor(&self) -> u32 {
        self.lexical.offset(self.cursor)
    }

    pub(crate) fn cursor(&self) -> usize {
        self.cursor
    }

    pub(crate) fn nth_nontrivia_kind(&self, index: usize) -> Option<TokenKind> {
        let mut cursor = self.cursor;
        for _ in 0..index {
            self.nth_nontrivia_kind_from(&mut cursor)?;
        }
        self.nth_nontrivia_kind_from(&mut cursor)
    }

    pub(crate) fn nth_nontrivia_kind_from(&self, cursor: &mut usize) -> Option<TokenKind> {
        if self.stopped.is_some() {
            return None;
        }
        while *cursor < self.lexical.len() {
            // The physical index survives repeated short lookahead calls. Do not
            // restart a local cadence for every skipped trivia run.
            if cursor.is_multiple_of(CANCELLATION_CHECK_TOKENS) {
                let result = self.cancel.check();
                #[cfg(test)]
                checkpoint(Checkpoint::Lookahead, *cursor);
                result.ok()?;
            }
            let kind = self.lexical.kind(*cursor);
            *cursor += 1;
            if !kind.is_trivia() {
                return Some(kind);
            }
        }
        None
    }

    pub(crate) fn joint_shift(&self) -> Option<SyntaxKind> {
        if self.stopped.is_some() {
            return None;
        }
        let first = self.lexical.kind(self.cursor);
        let second = self.lexical.kind(self.cursor + 1);
        match (first, second) {
            (TokenKind::Lt, TokenKind::Lt) => Some(SyntaxKind::Shl),
            (TokenKind::Gt, TokenKind::Gt) => Some(SyntaxKind::Shr),
            (TokenKind::Lt, TokenKind::Le) => Some(SyntaxKind::ShlEq),
            (TokenKind::Gt, TokenKind::Ge) => Some(SyntaxKind::ShrEq),
            _ => None,
        }
    }

    /// One list implementation preserves missing entries and punctuation. The
    /// caller supplies grammar admission and owns the sparse recovery field slots.
    /// `start_index` counts entries already parsed into the owning list.
    pub(crate) fn list<T, S>(
        &mut self,
        end: TokenKind,
        starts: fn(&Self) -> bool,
        parse: fn(&mut Self) -> Built<T>,
        field: u16,
        start_index: usize,
    ) -> (SeparatedList<T, S>, usize) {
        let mut result = Vec::new();
        let mut depth = 0;
        while !self.at(end.clone()) && !self.at(TokenKind::Eof) && self.continue_parsing() {
            let index = start_index + result.len();
            self.position(field, index);
            let before = self.cursor;
            let value = parse(self);
            depth = depth.max(value.depth);
            self.position(field, index);
            self.frames.last_mut().expect("list owner").part = RecoveryPart::Separator;
            while self.at(TokenKind::Unknown) && self.continue_parsing() {
                self.error_here(DiagnosticKind::UnexpectedToken);
                self.bump_as_error();
            }
            let separator = if let Some(comma) = self.eat(TokenKind::Comma) {
                Some(Required::Present(comma))
            } else if !self.at(end.clone()) && starts(self) {
                self.error_here(DiagnosticKind::UnexpectedToken);
                Some(Required::Missing { at: self.anchor() })
            } else {
                None
            };
            let continue_list = separator.is_some();
            result.push(ListEntry {
                element: value.node,
                separator,
            });
            if !continue_list {
                break;
            }
            if self.cursor == before && !self.at(TokenKind::Eof) {
                self.bump_as_error();
            }
        }

        (result.into(), depth)
    }
}
