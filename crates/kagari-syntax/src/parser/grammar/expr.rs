//! Precedence climbing, postfix ownership and explicit condition/string alternatives.

use kagari_source::diagnostic::DiagnosticKind;

use crate::{
    ast::{
        expr::{
            AppliedPath, ArrayContents, ArrayExpr, AsyncBlockExpr, AwaitExpr, BinaryExpr, BinaryOp,
            BindingCondition, BlockExpr, CallApplication, CallExpr, CastExpr, ClosureExpr,
            ClosureParam, ClosureParamList, ClosureParams, Condition, DebugMarker, ElseClause,
            Expr, FieldExpr, FieldInit, FieldInitList, FieldValue, IfExpr, IndexExpr,
            InterpolatedString, Interpolation, Literal, LoopExpr, MatchArm, MatchArmList,
            MatchExpr, MatchGuard, ParenExpr, PathExpr, PathMember, PrefixExpr, PrefixOp,
            PropagateExpr, RangeExpr, RangeOp, StringPart, StructEntry, StructExpr,
            StructUpdateBase, TupleExpr,
        },
        site::{ListEntry, Required},
        tokens,
    },
    kind::SyntaxKind,
    parser::core::{Built, Parser},
    token::TokenKind,
};

impl Parser<'_> {
    pub(crate) fn expr_starts(&self) -> bool {
        matches!(
            self.current_kind(),
            TokenKind::Ident
                | TokenKind::Lt
                | TokenKind::CrateKw
                | TokenKind::SelfKw
                | TokenKind::SuperKw
                | TokenKind::Number
                | TokenKind::Float
                | TokenKind::String
                | TokenKind::Char
                | TokenKind::ByteChar
                | TokenKind::ByteString
                | TokenKind::FormatStart
                | TokenKind::TrueKw
                | TokenKind::FalseKw
                | TokenKind::IfKw
                | TokenKind::MatchKw
                | TokenKind::LoopKw
                | TokenKind::LParen
                | TokenKind::LBracket
                | TokenKind::Minus
                | TokenKind::Bang
                | TokenKind::Pipe
                | TokenKind::PipePipe
                | TokenKind::AsyncKw
                | TokenKind::DotDot
                | TokenKind::DotDotEq
                | TokenKind::LBrace
        )
    }

    pub(crate) fn parse_expr(&mut self) -> Built<Expr> {
        self.with_nesting(Self::parse_range)
            .unwrap_or_else(|| Built::missing(self.anchor()))
    }

    pub(crate) fn parse_condition_expr(&mut self) -> Built<Expr> {
        self.with_struct_literals(false, Self::parse_expr)
    }

    /// Delimited expressions restore ordinary construction without changing the outer condition.
    pub(crate) fn with_struct_literals<T>(
        &mut self,
        allowed: bool,
        parse: impl FnOnce(&mut Self) -> T,
    ) -> T {
        let previous = self.allow_struct_literals;
        self.allow_struct_literals = allowed;
        let result = parse(self);
        self.allow_struct_literals = previous;
        result
    }

    pub(crate) fn parse_condition(&mut self) -> Built<Condition> {
        if self.at(TokenKind::ValKw) {
            let mark = self.begin();
            let keyword = self.take();
            let pattern = self.parse_match_pattern();
            let equal = self.expect(TokenKind::Eq, DiagnosticKind::UnexpectedToken);
            let initializer = self.parse_condition_expr();
            let site = self.end(mark);
            Built::present(
                Condition::Binding(Box::new(BindingCondition {
                    site,
                    keyword,
                    pattern: pattern.node,
                    equal,
                    initializer: initializer.node,
                })),
                self.completed_depth,
            )
        } else {
            let expression = self.parse_condition_expr();
            Built {
                node: match expression.node {
                    Required::Present(expr) => Required::Present(Condition::Expression(expr)),
                    Required::Missing { at } => Required::Missing { at },
                },
                depth: expression.depth,
            }
        }
    }

    /// A lone block is the recovered body; a block followed by a body remains a condition.
    pub(crate) fn parse_condition_body(&mut self) -> (Built<Condition>, Built<BlockExpr>) {
        let at = self.anchor();
        let condition = self.parse_condition();
        if !self.at(TokenKind::LBrace)
            && let Required::Present(Condition::Expression(Expr::BlockExpr(body))) = condition.node
        {
            self.error_here(DiagnosticKind::ExpectedExpression);
            return (Built::missing(at), Built::present(*body, condition.depth));
        }
        let body = self.parse_block();
        (condition, body)
    }

    fn parse_range(&mut self) -> Built<Expr> {
        let lhs = if self.at_any(&[TokenKind::DotDot, TokenKind::DotDotEq]) {
            None
        } else {
            Some(self.parse_binary(0))
        };
        if !self.at_any(&[TokenKind::DotDot, TokenKind::DotDotEq]) {
            return lhs.unwrap_or_else(|| Built::missing(self.anchor()));
        }
        let first = lhs
            .as_ref()
            .map_or(self.cursor() as u32, |lhs| self.first(lhs, self.cursor()));
        let mark = self.begin_at(first, lhs.as_ref().map_or(0, |lhs| lhs.depth));
        let inclusive = self.at(TokenKind::DotDotEq);
        let operator = self.operator(
            if inclusive {
                RangeOp::Inclusive
            } else {
                RangeOp::Exclusive
            },
            1,
        );
        let rhs = (inclusive
            || self.expr_starts() && (!self.at(TokenKind::LBrace) || self.allow_struct_literals))
            .then(|| self.parse_binary(0));
        let site = self.end(mark);
        Built::present(
            Expr::RangeExpr(Box::new(RangeExpr {
                site,
                start: lhs.and_then(|value| match value.node {
                    Required::Present(value) => Some(value),
                    Required::Missing { .. } => None,
                }),
                operator,
                end: rhs.map(|value| value.node),
            })),
            self.completed_depth,
        )
    }

    fn binary_operator(&self) -> Option<(u8, BinaryOp, usize)> {
        let (precedence, operator) = match self.current_kind() {
            TokenKind::PipePipe => (0, BinaryOp::Or),
            TokenKind::AmpAmp => (1, BinaryOp::And),
            TokenKind::EqEq => (2, BinaryOp::Equal),
            TokenKind::NotEq => (2, BinaryOp::NotEqual),
            TokenKind::IdentityEq => (2, BinaryOp::IdentityEqual),
            TokenKind::IdentityNotEq => (2, BinaryOp::IdentityNotEqual),
            TokenKind::Lt if self.joint_shift() == Some(SyntaxKind::Shl) => {
                return Some((8, BinaryOp::ShiftLeft, 2));
            }
            TokenKind::Gt if self.joint_shift() == Some(SyntaxKind::Shr) => {
                return Some((8, BinaryOp::ShiftRight, 2));
            }
            TokenKind::Lt if self.joint_shift().is_none() => (3, BinaryOp::Less),
            TokenKind::Le => (3, BinaryOp::LessEqual),
            TokenKind::Gt if self.joint_shift().is_none() => (3, BinaryOp::Greater),
            TokenKind::Ge => (3, BinaryOp::GreaterEqual),
            TokenKind::Pipe => (4, BinaryOp::BitOr),
            TokenKind::Caret => (5, BinaryOp::BitXor),
            TokenKind::Amp => (6, BinaryOp::BitAnd),
            TokenKind::Plus => (9, BinaryOp::Add),
            TokenKind::Minus => (9, BinaryOp::Subtract),
            TokenKind::Star => (10, BinaryOp::Multiply),
            TokenKind::Slash => (10, BinaryOp::Divide),
            TokenKind::Percent => (10, BinaryOp::Remainder),
            _ => return None,
        };
        Some((precedence, operator, 1))
    }

    fn parse_binary(&mut self, minimum: u8) -> Built<Expr> {
        let mut lhs = self.parse_cast();
        while let Some((precedence, operation, pieces)) = self.binary_operator() {
            if precedence < minimum || !self.continue_parsing() {
                break;
            }
            let suffix = self.cursor();
            if !self.admit(lhs.depth + 1, suffix) {
                break;
            }
            let mark = self.begin_at(self.first(&lhs, suffix), lhs.depth);
            let operator = Required::Present(self.operator(operation, pieces));
            let rhs = self.parse_binary(precedence + 1);
            let depth = lhs.depth.max(rhs.depth) + 1;
            if !self.admit(depth, suffix) {
                self.abandon(mark, suffix);
                break;
            }
            let site = self.end(mark);
            lhs = Built::present(
                Expr::BinaryExpr(Box::new(BinaryExpr {
                    site,
                    lhs: lhs.node,
                    operator,
                    rhs: rhs.node,
                })),
                self.completed_depth,
            );
        }
        lhs
    }

    fn parse_cast(&mut self) -> Built<Expr> {
        let mut expression = self.parse_prefix();
        while self.at(TokenKind::AsKw) && self.continue_parsing() {
            let suffix = self.cursor();
            if !self.admit(expression.depth + 1, suffix) {
                break;
            }
            let mark = self.begin_at(self.first(&expression, suffix), expression.depth);
            let keyword = self.take();
            let ty = self.parse_type_ref();
            let depth = expression.depth.max(ty.depth) + 1;
            if !self.admit(depth, suffix) {
                self.abandon(mark, suffix);
                break;
            }
            let site = self.end(mark);
            expression = Built::present(
                Expr::CastExpr(Box::new(CastExpr {
                    site,
                    expression: expression.node,
                    keyword,
                    ty: ty.node,
                })),
                self.completed_depth,
            );
        }
        expression
    }

    fn parse_prefix(&mut self) -> Built<Expr> {
        self.with_nesting(Self::parse_prefix_inner)
            .unwrap_or_else(|| Built::missing(self.anchor()))
    }

    fn parse_prefix_inner(&mut self) -> Built<Expr> {
        if self.at_any(&[TokenKind::Minus, TokenKind::Bang]) {
            let mark = self.begin();
            let operation = if self.at(TokenKind::Minus) {
                PrefixOp::Negate
            } else {
                PrefixOp::Not
            };
            let operator = Required::Present(self.operator(operation, 1));
            let expression = self.parse_prefix();
            let site = self.end(mark);
            Built::present(
                Expr::PrefixExpr(Box::new(PrefixExpr {
                    site,
                    operator,
                    expression: expression.node,
                })),
                self.completed_depth,
            )
        } else {
            self.parse_postfix()
        }
    }

    fn parse_postfix(&mut self) -> Built<Expr> {
        let mut lhs = self.parse_atom();
        loop {
            let suffix = self.cursor();
            if !self.at_any(&[
                TokenKind::LParen,
                TokenKind::ColonColon,
                TokenKind::Dot,
                TokenKind::Question,
                TokenKind::LBracket,
            ]) || self.at(TokenKind::ColonColon)
                && self.nth_nontrivia_kind(1) != Some(TokenKind::Lt)
            {
                break;
            }
            if !self.continue_parsing() || !self.admit(lhs.depth + 1, suffix) {
                break;
            }
            let mark = self.begin_at(self.first(&lhs, suffix), lhs.depth);
            let expression = match self.current_kind() {
                TokenKind::LParen | TokenKind::ColonColon => {
                    let mut application_depth = 0;
                    let application =
                        self.eat::<tokens::ColonColon>(TokenKind::ColonColon)
                            .map(|separator| {
                                let (arguments, depth) = self.parse_call_generic_arg_list();
                                application_depth = depth;
                                Box::new(CallApplication {
                                    separator: Some(separator),
                                    arguments,
                                })
                            });
                    let open = self.expect(TokenKind::LParen, DiagnosticKind::ExpectedClosingParen);
                    let allow_struct_literals =
                        open.as_ref().is_some() || self.allow_struct_literals;
                    let (arguments, argument_depth) =
                        self.with_struct_literals(allow_struct_literals, |parser| {
                            parser.list(
                                TokenKind::RParen,
                                Self::expr_starts,
                                Self::parse_expr,
                                3,
                                0,
                            )
                        });
                    let close =
                        self.expect(TokenKind::RParen, DiagnosticKind::ExpectedClosingParen);
                    if !self.admit(
                        lhs.depth.max(argument_depth).max(application_depth) + 1,
                        suffix,
                    ) {
                        self.abandon(mark, suffix);
                        break;
                    }
                    let site = self.end(mark);
                    Expr::CallExpr(Box::new(CallExpr {
                        site,
                        callee: lhs.node,
                        application,
                        open,
                        arguments,
                        close,
                    }))
                }
                TokenKind::Dot => {
                    let dot = self.take();
                    if self.at(TokenKind::AwaitKw) {
                        let keyword = Required::Present(self.take());
                        let site = self.end(mark);
                        Expr::AwaitExpr(Box::new(AwaitExpr {
                            site,
                            expression: lhs.node,
                            dot,
                            keyword,
                        }))
                    } else {
                        let name = self.parse_name(DiagnosticKind::ExpectedFieldName);
                        let site = self.end(mark);
                        Expr::FieldExpr(Box::new(FieldExpr {
                            site,
                            receiver: lhs.node,
                            dot,
                            name,
                        }))
                    }
                }
                TokenKind::Question => {
                    let question = self.take();
                    let site = self.end(mark);
                    Expr::PropagateExpr(Box::new(PropagateExpr {
                        site,
                        expression: lhs.node,
                        question,
                    }))
                }
                TokenKind::LBracket => {
                    let open = self.take();
                    let index = self.with_struct_literals(true, Self::parse_expr);
                    let close =
                        self.expect(TokenKind::RBracket, DiagnosticKind::ExpectedClosingBracket);
                    if !self.admit(lhs.depth.max(index.depth) + 1, suffix) {
                        self.abandon(mark, suffix);
                        break;
                    }
                    let site = self.end(mark);
                    Expr::IndexExpr(Box::new(IndexExpr {
                        site,
                        receiver: lhs.node,
                        open,
                        index: index.node,
                        close,
                    }))
                }
                _ => unreachable!("admitted postfix"),
            };
            lhs = Built::present(expression, self.completed_depth);
        }
        lhs
    }

    fn parse_atom(&mut self) -> Built<Expr> {
        match self.current_kind() {
            TokenKind::Ident
            | TokenKind::CrateKw
            | TokenKind::SelfKw
            | TokenKind::SuperKw
            | TokenKind::Lt => self.parse_path_or_struct(),
            TokenKind::Number
            | TokenKind::Float
            | TokenKind::String
            | TokenKind::Char
            | TokenKind::ByteChar
            | TokenKind::ByteString
            | TokenKind::TrueKw
            | TokenKind::FalseKw => {
                Built::present(Expr::Literal(self.parse_literal()), self.completed_depth)
            }
            TokenKind::LBrace => {
                let block = self.parse_block();
                Built {
                    depth: block.depth,
                    node: match block.node {
                        Required::Present(block) => {
                            Required::Present(Expr::BlockExpr(Box::new(block)))
                        }
                        Required::Missing { at } => Required::Missing { at },
                    },
                }
            }
            TokenKind::LParen => self.with_struct_literals(true, Self::parse_parenthesized),
            TokenKind::LBracket => self.with_struct_literals(true, Self::parse_array),
            TokenKind::IfKw => self.parse_if(),
            TokenKind::MatchKw => self.parse_match(),
            TokenKind::LoopKw => {
                let mark = self.begin();
                let keyword = self.take();
                let body = self.parse_block();
                let site = self.end(mark);
                Built::present(
                    Expr::LoopExpr(Box::new(LoopExpr {
                        site,
                        keyword,
                        body: body.node,
                    })),
                    self.completed_depth,
                )
            }
            TokenKind::AsyncKw
                if !matches!(
                    self.nth_nontrivia_kind(1),
                    Some(TokenKind::Pipe | TokenKind::PipePipe)
                ) =>
            {
                let mark = self.begin();
                let keyword = self.take();
                let body = if self.at(TokenKind::LBrace) {
                    self.parse_block()
                } else {
                    self.error_here(DiagnosticKind::ExpectedExpression);
                    Built::missing(self.anchor())
                };
                let site = self.end(mark);
                Built::present(
                    Expr::AsyncBlockExpr(Box::new(AsyncBlockExpr {
                        site,
                        keyword,
                        body: body.node,
                    })),
                    self.completed_depth,
                )
            }
            TokenKind::AsyncKw | TokenKind::Pipe | TokenKind::PipePipe => self.parse_closure(),
            TokenKind::FormatStart => self.parse_string(),
            TokenKind::Unknown => {
                let mark = self.begin();
                self.error_here(DiagnosticKind::ExpectedExpression);
                self.bump_as_error();
                Built::present(Expr::Error(self.end(mark)), self.completed_depth)
            }
            _ => {
                self.error_here(DiagnosticKind::ExpectedExpression);
                Built::missing(self.anchor())
            }
        }
    }

    pub(crate) fn parse_literal(&mut self) -> Literal {
        {
            let literal = Literal { token: self.take() };
            self.account_depth(1);
            literal
        }
    }

    pub(crate) fn parse_path_expr(&mut self) -> Built<PathExpr> {
        if self.at(TokenKind::Lt) {
            let mark = self.begin();
            let (qualified, _) = self.parse_qualified_type();
            self.end(mark);
            return Built::present(
                PathExpr::Qualified(Box::new(qualified)),
                self.completed_depth,
            );
        }
        let mark = self.begin();
        let path = self.parse_path();
        let mut cursor = self.cursor();
        let first = self.nth_nontrivia_kind_from(&mut cursor);
        let separator = first == Some(TokenKind::ColonColon);
        let angle = first == Some(TokenKind::Lt)
            || separator && self.nth_nontrivia_kind_from(&mut cursor) == Some(TokenKind::Lt);
        let applied = angle
            && self.skip_angle_group(&mut cursor)
            && if separator {
                self.nth_nontrivia_kind_from(&mut cursor) != Some(TokenKind::LParen)
            } else {
                self.nth_nontrivia_kind_from(&mut cursor) == Some(TokenKind::ColonColon)
            };
        if applied {
            let application_separator = if separator { Some(self.take()) } else { None };
            let (arguments, _) = self.parse_generic_arg_list();
            let member = self.eat(TokenKind::ColonColon).map(|separator| PathMember {
                separator,
                name: self.parse_name(DiagnosticKind::ExpectedFunctionName),
            });
            let site = self.end(mark);
            Built::present(
                PathExpr::Applied(Box::new(AppliedPath {
                    site,
                    path,
                    application_separator,
                    arguments,
                    member,
                })),
                self.completed_depth,
            )
        } else {
            self.end(mark);
            Built {
                node: match path {
                    Required::Present(path) => Required::Present(PathExpr::Plain(path)),
                    Required::Missing { at } => Required::Missing { at },
                },
                depth: self.completed_depth,
            }
        }
    }

    fn parse_path_or_struct(&mut self) -> Built<Expr> {
        let path = self.parse_path_expr();
        let mut cursor = self.cursor();
        let generic_struct = self.allow_struct_literals
            && self.nth_nontrivia_kind_from(&mut cursor) == Some(TokenKind::Lt)
            && self.skip_angle_group(&mut cursor)
            && self.nth_nontrivia_kind_from(&mut cursor) == Some(TokenKind::LBrace);
        if self.allow_struct_literals && (generic_struct || self.at(TokenKind::LBrace)) {
            let mark = self.begin_at(self.first(&path, self.cursor()), path.depth);
            let arguments = generic_struct.then(|| Box::new(self.parse_generic_arg_list().0));
            let (fields, _) = self.parse_field_initializers();
            let site = self.end(mark);
            Built::present(
                Expr::StructExpr(Box::new(StructExpr {
                    site,
                    path: path.node,
                    arguments,
                    fields,
                })),
                self.completed_depth,
            )
        } else {
            Built {
                node: match path.node {
                    Required::Present(path) => Required::Present(Expr::PathExpr(Box::new(path))),
                    Required::Missing { at } => Required::Missing { at },
                },
                depth: path.depth,
            }
        }
    }

    fn parse_parenthesized(&mut self) -> Built<Expr> {
        let mark = self.begin();
        let open = self.take();
        let first = (!self.at(TokenKind::RParen)).then(|| self.parse_expr());
        match (first, self.at(TokenKind::Comma)) {
            (Some(first), false) => {
                let close = self.expect(TokenKind::RParen, DiagnosticKind::ExpectedClosingParen);
                let site = self.end(mark);
                Built::present(
                    Expr::ParenExpr(Box::new(ParenExpr {
                        site,
                        open,
                        expression: first.node,
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
                    let (rest, _) =
                        self.list(TokenKind::RParen, Self::expr_starts, Self::parse_expr, 1, 1);
                    elements.extend(rest.entries);
                }
                let close = self.expect(TokenKind::RParen, DiagnosticKind::ExpectedClosingParen);
                let site = self.end(mark);
                Built::present(
                    Expr::TupleExpr(Box::new(TupleExpr {
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

    fn parse_array(&mut self) -> Built<Expr> {
        let mark = self.begin();
        let open = self.take();
        let first = (!self.at(TokenKind::RBracket)).then(|| self.parse_expr());
        let contents = if let Some(separator) = self.eat(TokenKind::Semi) {
            let count = self.parse_expr();
            ArrayContents::Repeat {
                value: first.map_or_else(
                    || Required::Missing { at: self.anchor() },
                    |first| first.node,
                ),
                separator,
                count: count.node,
            }
        } else {
            let mut elements = Vec::new();
            if let Some(first) = first {
                let separator = self.eat(TokenKind::Comma).map(Required::Present);
                let more = separator.is_some();
                elements.push(ListEntry {
                    element: first.node,
                    separator,
                });
                if more {
                    let (rest, _) = self.list(
                        TokenKind::RBracket,
                        Self::expr_starts,
                        Self::parse_expr,
                        1,
                        1,
                    );
                    elements.extend(rest.entries);
                }
            }
            ArrayContents::List(elements.into())
        };
        let close = self.expect(TokenKind::RBracket, DiagnosticKind::ExpectedClosingBracket);
        let site = self.end(mark);
        Built::present(
            Expr::ArrayExpr(Box::new(ArrayExpr {
                site,
                open,
                contents,
                close,
            })),
            self.completed_depth,
        )
    }

    fn parse_if(&mut self) -> Built<Expr> {
        self.with_nesting(Self::parse_if_inner)
            .unwrap_or_else(|| Built::missing(self.anchor()))
    }

    fn parse_if_inner(&mut self) -> Built<Expr> {
        let mark = self.begin();
        let keyword = self.take();
        let (condition, body) = self.parse_condition_body();
        let then_body = match body.node {
            Required::Present(body) => Required::Present(Box::new(body)),
            Required::Missing { at } => Required::Missing { at },
        };
        let else_clause = self.eat(TokenKind::ElseKw).map(|keyword| {
            let expression = if self.at(TokenKind::IfKw) {
                self.parse_if()
            } else if self.at(TokenKind::LBrace) {
                self.parse_atom()
            } else {
                self.error_here(DiagnosticKind::ExpectedExpression);
                Built::missing(self.anchor())
            };
            ElseClause {
                keyword,
                body: expression.node,
            }
        });
        let site = self.end(mark);
        Built::present(
            Expr::IfExpr(Box::new(IfExpr {
                site,
                keyword,
                condition: condition.node,
                then_body,
                else_clause,
            })),
            self.completed_depth,
        )
    }

    fn parse_match(&mut self) -> Built<Expr> {
        let mark = self.begin();
        let keyword = self.take();
        let scrutinee = self.parse_condition_expr();
        let arms_mark = self.begin();
        let open = self.expect(
            TokenKind::LBrace,
            DiagnosticKind::ExpectedStructLiteralBodyStart,
        );
        let allow_struct_literals = open.as_ref().is_some() || self.allow_struct_literals;
        let (arms, _) = self.with_struct_literals(allow_struct_literals, |parser| {
            parser.list(
                TokenKind::RBrace,
                Self::pattern_starts,
                Self::parse_match_arm,
                1,
                0,
            )
        });
        let close = self.expect(TokenKind::RBrace, DiagnosticKind::ExpectedBlockEnd);
        let site = self.end(arms_mark);
        let arms = MatchArmList {
            site,
            open,
            arms,
            close,
        };
        let site = self.end(mark);
        Built::present(
            Expr::MatchExpr(Box::new(MatchExpr {
                site,
                keyword,
                scrutinee: scrutinee.node,
                arms,
            })),
            self.completed_depth,
        )
    }

    fn parse_match_arm(&mut self) -> Built<MatchArm> {
        let mark = self.begin();
        let pattern = self.parse_match_pattern();
        let guard = self.eat(TokenKind::IfKw).map(|keyword| {
            let expression = self.parse_expr();
            MatchGuard {
                keyword,
                expression: expression.node,
            }
        });
        let arrow = self.expect(TokenKind::FatArrow, DiagnosticKind::UnexpectedToken);
        let expression = self.parse_expr();
        let site = self.end(mark);
        Built::present(
            MatchArm {
                site,
                pattern: pattern.node,
                guard,
                arrow,
                expression: expression.node,
            },
            self.completed_depth,
        )
    }

    fn closure_param_starts(&self) -> bool {
        self.at(TokenKind::Ident)
    }

    fn parse_closure_param(&mut self) -> Built<ClosureParam> {
        let mark = self.begin();
        let name = self.parse_name(DiagnosticKind::ExpectedFunctionName);
        let annotation = self.parse_annotation();
        let site = self.end(mark);
        Built::present(
            ClosureParam {
                site,
                name,
                annotation,
            },
            self.completed_depth,
        )
    }

    fn parse_closure(&mut self) -> Built<Expr> {
        let mark = self.begin();
        let async_keyword = self.eat(TokenKind::AsyncKw);
        let parameters = if self.at(TokenKind::PipePipe) {
            Required::Present(ClosureParams::Empty(self.take()))
        } else if self.at(TokenKind::Pipe) {
            let list_mark = self.begin();
            let open = self.take();
            let (parameters, _) = self.list(
                TokenKind::Pipe,
                Self::closure_param_starts,
                Self::parse_closure_param,
                1,
                0,
            );
            let close = self.expect(TokenKind::Pipe, DiagnosticKind::ExpectedExpression);
            let site = self.end(list_mark);
            Required::Present(ClosureParams::List(ClosureParamList {
                site,
                open,
                parameters,
                close,
            }))
        } else {
            self.error_here(DiagnosticKind::ExpectedExpression);
            Required::Missing { at: self.anchor() }
        };
        let body = self.parse_expr();
        let site = self.end(mark);
        Built::present(
            Expr::ClosureExpr(Box::new(ClosureExpr {
                site,
                async_keyword,
                parameters,
                body: body.node,
            })),
            self.completed_depth,
        )
    }

    fn parse_string(&mut self) -> Built<Expr> {
        let mark = self.begin();
        let open = self.take();
        let mut parts = Vec::new();
        while !self.at_any(&[TokenKind::FormatEnd, TokenKind::Eof]) && self.continue_parsing() {
            self.position(1, parts.len());
            if self.at(TokenKind::FormatText) {
                parts.push(StringPart::Text(self.take()));
            } else if self.at(TokenKind::FormatOpen) {
                let hole_mark = self.begin();
                let open = self.take();
                self.position(1, 0);
                let expression = if self.at(TokenKind::Ident) {
                    self.parse_path_expr().node
                } else {
                    self.error_here(DiagnosticKind::InvalidLiteral { reason: "interpolation requires {name} or {name:?} without whitespace; bind expressions outside the String".into() });
                    while !self.at_any(&[
                        TokenKind::FormatClose,
                        TokenKind::FormatEnd,
                        TokenKind::Eof,
                    ]) && self.continue_parsing()
                    {
                        self.bump_as_error();
                    }
                    Required::Missing { at: self.anchor() }
                };
                let debug = self.eat(TokenKind::Colon).map(|colon| DebugMarker {
                    colon,
                    question: self.expect(TokenKind::Question, DiagnosticKind::UnexpectedToken),
                });
                let close = self.expect(
                    TokenKind::FormatClose,
                    DiagnosticKind::InvalidLiteral {
                        reason: "unterminated interpolation; expected `}`".into(),
                    },
                );
                let site = self.end(hole_mark);
                parts.push(StringPart::Hole(Interpolation {
                    site,
                    open,
                    expression,
                    debug,
                    close,
                }));
            } else {
                self.error_here(DiagnosticKind::UnexpectedToken);
                self.bump_as_error();
            }
        }
        parts.shrink_to_fit();
        let close = self.expect(
            TokenKind::FormatEnd,
            DiagnosticKind::InvalidLiteral {
                reason: "unterminated String literal; expected closing quote".into(),
            },
        );
        let site = self.end(mark);
        Built::present(
            Expr::InterpolatedString(Box::new(InterpolatedString {
                site,
                open,
                parts,
                close,
            })),
            self.completed_depth,
        )
    }

    fn parse_struct_entry(&mut self) -> Built<StructEntry> {
        let mark = self.begin();
        if self.at(TokenKind::DotDot) {
            let dots = self.take();
            let value =
                if self.expr_starts() && !self.at_any(&[TokenKind::DotDot, TokenKind::DotDotEq]) {
                    self.parse_expr()
                } else {
                    self.error_here(DiagnosticKind::ExpectedExpression);
                    Built::missing(self.anchor())
                };
            let site = self.end(mark);
            Built::present(
                StructEntry::Update(StructUpdateBase {
                    site,
                    dots,
                    value: value.node,
                }),
                self.completed_depth,
            )
        } else {
            let name = self.parse_name(DiagnosticKind::ExpectedFieldName);
            let value = self.eat(TokenKind::Colon).map(|colon| {
                let expression = self.parse_expr();
                FieldValue {
                    colon,
                    expression: expression.node,
                }
            });
            let site = self.end(mark);
            Built::present(
                StructEntry::Field(FieldInit { site, name, value }),
                self.completed_depth,
            )
        }
    }

    fn parse_field_initializers(&mut self) -> (FieldInitList, usize) {
        let mark = self.begin();
        let open = self.expect(
            TokenKind::LBrace,
            DiagnosticKind::ExpectedStructLiteralBodyStart,
        );
        let mut entries = Vec::new();
        while self.at(TokenKind::Ident) && self.continue_parsing() {
            self.position(1, entries.len());
            let field = self.parse_struct_entry();
            let separator = self.eat(TokenKind::Comma).map(Required::Present);
            let more = separator.is_some();
            entries.push(ListEntry {
                element: field.node,
                separator,
            });
            if !more {
                if self.at_any(&[TokenKind::DotDot, TokenKind::DotDotEq]) {
                    self.error_here(DiagnosticKind::UnexpectedToken);
                }
                break;
            }
        }
        if self.at_any(&[TokenKind::DotDot, TokenKind::DotDotEq]) {
            self.position(1, entries.len());
            if self.at(TokenKind::DotDot) {
                let base = self.parse_struct_entry();
                entries.push(ListEntry {
                    element: base.node,
                    separator: None,
                });
            } else {
                self.error_here(DiagnosticKind::UnexpectedToken);
                self.bump_as_error();
                if self.expr_starts() {
                    self.rejected(Self::parse_expr);
                }
            }
            self.reject_struct_update_tail(entries.len());
        }

        let close = self.expect(TokenKind::RBrace, DiagnosticKind::ExpectedBlockEnd);
        let site = self.end(mark);
        (
            FieldInitList {
                site,
                open,
                entries: entries.into(),
                close,
            },
            self.completed_depth,
        )
    }

    /// Rejects fields, extra updates and commas after the attempted final update.
    /// The open `FieldInitList` frame owns this input at `entries[entry]`, just
    /// after its retained entries; rejected nodes never enter that typed list.
    /// Stops before the closing brace or an unrecognized tail for the caller.
    fn reject_struct_update_tail(&mut self, entry: usize) {
        while self.continue_parsing() {
            self.position(1, entry);
            let comma = self.at(TokenKind::Comma);
            if comma {
                self.error_here(DiagnosticKind::UnexpectedToken);
                self.bump_as_error();
            }
            if self.at(TokenKind::Ident)
                && (comma
                    || matches!(
                        self.nth_nontrivia_kind(1),
                        Some(TokenKind::Colon | TokenKind::Comma | TokenKind::RBrace)
                    ))
            {
                if !comma {
                    self.error_here(DiagnosticKind::UnexpectedToken);
                }
                self.rejected(Self::parse_struct_entry);
            } else if self.at_any(&[TokenKind::DotDot, TokenKind::DotDotEq]) {
                self.error_here(DiagnosticKind::UnexpectedToken);
                if self.at(TokenKind::DotDot) {
                    self.rejected(Self::parse_struct_entry);
                } else {
                    self.bump_as_error();
                    if self.expr_starts() {
                        self.rejected(Self::parse_expr);
                    }
                }
            } else {
                break;
            }
        }
    }
}
