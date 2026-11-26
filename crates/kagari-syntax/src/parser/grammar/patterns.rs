//! Written pattern alternatives; binding/capture and place semantics remain in HIR.

use kagari_source::diagnostic::DiagnosticKind;

use crate::{
    ast::{
        expr::{
            FieldPattern, GroupedPattern, OrPattern, PathExpr, Pattern, PatternAlternative,
            PatternBound, PatternField, PatternLiteral, PatternRest, PositionalPattern, RangeOp,
            RangePattern, StructPattern, TuplePattern, TupleStructPattern,
        },
        site::{ListEntry, Required},
    },
    parser::core::{Built, Parser},
    token::TokenKind,
};

impl Parser<'_> {
    pub(crate) fn pattern_starts(&self) -> bool {
        self.path_starts() || self.at(TokenKind::LParen) || self.pattern_literal_starts()
    }

    fn pattern_literal_starts(&self) -> bool {
        self.negative_literal_starts()
            || self.at_any(&[
                TokenKind::Number,
                TokenKind::Float,
                TokenKind::String,
                TokenKind::Char,
                TokenKind::ByteChar,
                TokenKind::TrueKw,
                TokenKind::FalseKw,
            ])
    }

    fn negative_literal_starts(&self) -> bool {
        self.at(TokenKind::Minus)
            && matches!(
                self.nth_nontrivia_kind(1),
                Some(TokenKind::Number | TokenKind::Float)
            )
    }

    fn parse_pattern_literal(&mut self) -> PatternLiteral {
        PatternLiteral {
            minus: self.eat(TokenKind::Minus),
            value: self.parse_literal(),
        }
    }

    fn positional_starts(&self) -> bool {
        self.at(TokenKind::DotDot) || self.pattern_starts()
    }

    fn parse_positional(&mut self, saw_rest: &mut bool) -> Built<PositionalPattern> {
        if self.at(TokenKind::DotDot) {
            if *saw_rest {
                self.error_here(DiagnosticKind::UnexpectedToken);
            }
            *saw_rest = true;
            let dots = self.take();
            self.account_depth(1);
            Built::present(PositionalPattern::Rest(dots), self.completed_depth)
        } else {
            let pattern = self.parse_match_pattern();
            Built {
                node: pattern.node.map(PositionalPattern::Pattern),
                depth: pattern.depth,
            }
        }
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
        while self.at(TokenKind::Pipe) && self.continue_parsing() {
            let separator = self.take();
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
                    let mut saw_rest = false;
                    let (elements, _) = self.list(
                        TokenKind::RParen,
                        Self::positional_starts,
                        |parser| parser.parse_positional(&mut saw_rest),
                        2,
                        0,
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
            _ if self.pattern_literal_starts() => {
                let mark = self.begin();
                let literal = self.parse_pattern_literal();
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
        let first_token = self.first(&first, self.cursor());
        let operator_start = self.cursor();
        let mark = self.begin_at(first_token, first.depth.saturating_sub(1));
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
                // RangePattern.start cannot own a compound pattern or wildcard.
                // Retain the rejected prefix before the operator in traversal.
                self.unexpected_range(first_token as usize, operator_start);
                self.error_here(DiagnosticKind::ExpectedMatchPattern);
                Required::Missing { at: self.anchor() }
            }
        };
        let end = if self.path_starts() {
            match self.parse_path_expr().node {
                Required::Present(value) => Required::Present(PatternBound::Path(value)),
                Required::Missing { at } => Required::Missing { at },
            }
        } else if self.pattern_literal_starts() {
            Required::Present(PatternBound::Literal(self.parse_pattern_literal()))
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
        let mut saw_rest = false;
        let first = (!self.at(TokenKind::RParen)).then(|| self.parse_positional(&mut saw_rest));
        match (first, self.at(TokenKind::Comma) || saw_rest) {
            (Some(first), false) => {
                let close = self.expect(TokenKind::RParen, DiagnosticKind::ExpectedClosingParen);
                let site = self.end(mark);
                Built::present(
                    Pattern::Grouped(Box::new(GroupedPattern {
                        site,
                        open,
                        pattern: first.node.map(|entry| match entry {
                            PositionalPattern::Pattern(pattern) => pattern,
                            PositionalPattern::Rest(_) => unreachable!("rest selects tuple syntax"),
                        }),
                        close,
                    })),
                    self.completed_depth,
                )
            }
            (first, _) => {
                let mut elements = Vec::new();
                if let Some(first) = first {
                    let separator =
                        self.eat(TokenKind::Comma)
                            .map(Required::Present)
                            .or_else(|| {
                                if self.positional_starts() {
                                    self.error_here(DiagnosticKind::UnexpectedToken);
                                    Some(Required::Missing { at: self.anchor() })
                                } else {
                                    None
                                }
                            });
                    elements.push(ListEntry {
                        element: first.node,
                        separator,
                    });
                    let (rest, _) = self.list(
                        TokenKind::RParen,
                        Self::positional_starts,
                        |parser| parser.parse_positional(&mut saw_rest),
                        1,
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
        while !self.at_any(&[TokenKind::RBrace, TokenKind::Eof]) && self.continue_parsing() {
            if self.at(TokenKind::DotDot) {
                if rest.is_some() {
                    self.position(4, 0); // StructPattern.close owns the rejected suffix.
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
                    self.position(4, 0);
                    self.error_here(DiagnosticKind::UnexpectedToken);
                    self.rejected(Self::parse_pattern_field);
                    if self.at(TokenKind::Comma) {
                        self.bump_as_error();
                        continue;
                    }
                    break;
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
