//! Written pattern alternatives; binding/capture and place semantics remain in HIR.

use kagari_source::diagnostic::DiagnosticKind;

use crate::{
    ast::{
        expr::{
            FieldPattern, GroupedPattern, OrPattern, PathExpr, Pattern, PatternAlternative,
            PatternBound, PatternField, PatternRest, RangeOp, RangePattern, StructPattern,
            TuplePattern, TupleStructPattern,
        },
        site::{ListEntry, Required},
    },
    parser::core::{Built, Parser},
    token::TokenKind,
};

impl Parser<'_> {
    pub(crate) fn pattern_starts(&self) -> bool {
        self.path_starts()
            || self.at_any(&[
                TokenKind::LParen,
                TokenKind::Number,
                TokenKind::Float,
                TokenKind::String,
                TokenKind::Char,
                TokenKind::ByteChar,
                TokenKind::TrueKw,
                TokenKind::FalseKw,
            ])
    }

    pub(crate) fn parse_binding_pattern(&mut self) -> Built<Pattern> {
        self.with_nesting(Self::parse_single_pattern)
            .unwrap_or_else(|| Built::missing(self.anchor()))
    }

    pub(crate) fn parse_match_pattern(&mut self) -> Built<Pattern> {
        self.with_nesting(Self::parse_pattern_alternatives)
            .unwrap_or_else(|| Built::missing(self.anchor()))
    }

    fn parse_pattern_alternatives(&mut self) -> Built<Pattern> {
        let first = self.parse_single_pattern();
        if !self.at(TokenKind::Pipe) {
            return first;
        }
        let mark = self.begin_at(self.first(&first, self.cursor()), first.depth);
        let mut alternatives = Vec::new();
        while let Some(separator) = self.eat(TokenKind::Pipe) {
            let pattern = self.parse_single_pattern();
            alternatives.push(PatternAlternative {
                separator,
                pattern: pattern.node,
            });
        }
        alternatives.shrink_to_fit();
        let site = self.end(mark);
        Built::present(
            Pattern::Or(Box::new(OrPattern {
                site,
                first: first.node,
                alternatives,
            })),
            self.completed_depth,
        )
    }

    fn parse_single_pattern(&mut self) -> Built<Pattern> {
        let first = match self.current_kind() {
            TokenKind::Ident if self.current_text() == "_" => {
                let token = self.take();
                self.account_depth(1);
                Built::present(Pattern::Wildcard(token), self.completed_depth)
            }
            _ if self.path_starts() => {
                let path = self.parse_path_expr();
                if self.at(TokenKind::LParen) {
                    let mark = self.begin_at(self.first(&path, self.cursor()), path.depth);
                    let open = self.take();
                    let (elements, _) = self.list(
                        TokenKind::RParen,
                        Self::pattern_starts,
                        Self::parse_match_pattern,
                        2,
                    );
                    let close =
                        self.expect(TokenKind::RParen, DiagnosticKind::ExpectedClosingParen);
                    let site = self.end(mark);
                    match path.node {
                        Required::Present(path) => Built::present(
                            Pattern::TupleStruct(Box::new(TupleStructPattern {
                                site,
                                path,
                                open,
                                elements,
                                close,
                            })),
                            self.completed_depth,
                        ),
                        Required::Missing { at } => Built::missing(at),
                    }
                } else if self.at(TokenKind::LBrace) {
                    self.parse_struct_pattern(path)
                } else {
                    let mark = self.begin_at(self.first(&path, self.cursor()), path.depth);
                    self.end(mark);
                    Built {
                        node: match path.node {
                            Required::Present(path) => {
                                Required::Present(Pattern::Path(Box::new(path)))
                            }
                            Required::Missing { at } => Required::Missing { at },
                        },
                        depth: self.completed_depth,
                    }
                }
            }
            TokenKind::Number
            | TokenKind::Float
            | TokenKind::String
            | TokenKind::Char
            | TokenKind::ByteChar
            | TokenKind::TrueKw
            | TokenKind::FalseKw => {
                let mark = self.begin();
                let literal = self.parse_literal();
                self.end(mark);
                Built::present(Pattern::Literal(literal), self.completed_depth)
            }
            TokenKind::LParen => self.parse_tuple_pattern(),
            _ => {
                self.error_here(DiagnosticKind::ExpectedMatchPattern);
                Built::missing(self.anchor())
            }
        };
        if !self.at_any(&[TokenKind::DotDot, TokenKind::DotDotEq]) {
            return first;
        }
        let mark = self.begin_at(
            self.first(&first, self.cursor()),
            first.depth.saturating_sub(1),
        );
        let operation = if self.at(TokenKind::DotDotEq) {
            RangeOp::Inclusive
        } else {
            RangeOp::Exclusive
        };
        let operator = self.operator(operation, 1);
        let start = match first.node {
            Required::Present(Pattern::Literal(value)) => {
                Required::Present(PatternBound::Literal(value))
            }
            Required::Present(Pattern::Path(value)) => {
                Required::Present(PatternBound::Path(*value))
            }
            Required::Missing { at } => Required::Missing { at },
            Required::Present(_) => {
                self.error_here(DiagnosticKind::ExpectedMatchPattern);
                Required::Missing { at: self.anchor() }
            }
        };
        let end = if self.path_starts() {
            match self.parse_path_expr().node {
                Required::Present(value) => Required::Present(PatternBound::Path(value)),
                Required::Missing { at } => Required::Missing { at },
            }
        } else if self.at_any(&[
            TokenKind::Number,
            TokenKind::Float,
            TokenKind::String,
            TokenKind::Char,
            TokenKind::ByteChar,
            TokenKind::TrueKw,
            TokenKind::FalseKw,
        ]) {
            Required::Present(PatternBound::Literal(self.parse_literal()))
        } else {
            self.error_here(DiagnosticKind::ExpectedMatchPattern);
            Required::Missing { at: self.anchor() }
        };
        let site = self.end(mark);
        Built::present(
            Pattern::Range(Box::new(RangePattern {
                site,
                start,
                operator,
                end,
            })),
            self.completed_depth,
        )
    }

    fn parse_tuple_pattern(&mut self) -> Built<Pattern> {
        let mark = self.begin();
        let open = self.take();
        let first = (!self.at(TokenKind::RParen)).then(|| self.parse_match_pattern());
        match (first, self.at(TokenKind::Comma)) {
            (Some(first), false) => {
                let close = self.expect(TokenKind::RParen, DiagnosticKind::ExpectedClosingParen);
                let site = self.end(mark);
                Built::present(
                    Pattern::Grouped(Box::new(GroupedPattern {
                        site,
                        open,
                        pattern: first.node,
                        close,
                    })),
                    self.completed_depth,
                )
            }
            (first, _) => {
                let mut elements = Vec::new();
                if let Some(first) = first {
                    let separator = self.eat(TokenKind::Comma).map(Required::Present);
                    elements.push(ListEntry {
                        element: first.node,
                        separator,
                    });
                    let (rest, _) = self.list(
                        TokenKind::RParen,
                        Self::pattern_starts,
                        Self::parse_match_pattern,
                        1,
                    );
                    elements.extend(rest.entries);
                }
                let close = self.expect(TokenKind::RParen, DiagnosticKind::ExpectedClosingParen);
                let site = self.end(mark);
                Built::present(
                    Pattern::Tuple(Box::new(TuplePattern {
                        site,
                        open,
                        elements: elements.into(),
                        close,
                    })),
                    self.completed_depth,
                )
            }
        }
    }

    fn parse_pattern_field(&mut self) -> Built<PatternField> {
        let mark = self.begin();
        let name = self.parse_name(DiagnosticKind::ExpectedFieldName);
        let pattern = self.eat(TokenKind::Colon).map(|colon| {
            let pattern = self.parse_match_pattern();
            FieldPattern {
                colon,
                pattern: pattern.node,
            }
        });
        let site = self.end(mark);
        Built::present(
            PatternField {
                site,
                name,
                pattern,
            },
            self.completed_depth,
        )
    }

    fn parse_struct_pattern(&mut self, path: Built<PathExpr>) -> Built<Pattern> {
        let mark = self.begin_at(self.first(&path, self.cursor()), path.depth);
        let open = self.take();
        let mut fields = Vec::new();
        let mut rest = None;
        while !self.at_any(&[TokenKind::RBrace, TokenKind::Eof]) {
            if self.at(TokenKind::DotDot) {
                if rest.is_some() {
                    self.error_here(DiagnosticKind::UnexpectedToken);
                    self.bump_as_error();
                    if self.at(TokenKind::Comma) {
                        self.bump_as_error();
                    }
                } else {
                    let dots = self.take();
                    let comma = self.eat(TokenKind::Comma);
                    rest = Some(PatternRest { dots, comma });
                }
            } else if self.at(TokenKind::Ident) {
                if rest.is_some() {
                    self.error_here(DiagnosticKind::UnexpectedToken);
                }
                let field = self.parse_pattern_field();
                let separator = self.eat(TokenKind::Comma).map(Required::Present);
                let more = separator.is_some();
                fields.push(ListEntry {
                    element: field.node,
                    separator,
                });
                if !more {
                    break;
                }
            } else {
                break;
            }
        }

        let close = self.expect(TokenKind::RBrace, DiagnosticKind::ExpectedBlockEnd);
        let site = self.end(mark);
        match path.node {
            Required::Present(path) => Built::present(
                Pattern::Struct(Box::new(StructPattern {
                    site,
                    path,
                    open,
                    fields: fields.into(),
                    rest,
                    close,
                })),
                self.completed_depth,
            ),
            Required::Missing { at } => Built::missing(at),
        }
    }
}
