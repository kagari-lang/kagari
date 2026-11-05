//! Attached attributes use the shared declaration/value grammar, with bounded nesting.

use kagari_source::diagnostic::DiagnosticKind;

use crate::{
    ast::{
        item::{
            Attribute, AttributeArg, AttributeArgs, AttributeKey, AttributeList, AttributePayload,
            AttributeValue, Attributes,
        },
        site::Required,
    },
    parser::core::{Built, Parser},
    token::TokenKind,
};

impl Parser<'_> {
    pub(crate) fn attributed_item_kind(&self) -> Option<TokenKind> {
        let mut cursor = self.cursor();
        let mut kind = self.nth_nontrivia_kind_from(&mut cursor)?;
        while kind == TokenKind::Hash {
            if self.nth_nontrivia_kind_from(&mut cursor)? != TokenKind::LBracket {
                return None;
            }
            kind = self.nth_nontrivia_kind_from(&mut cursor)?;
            if !matches!(
                kind,
                TokenKind::Ident | TokenKind::CrateKw | TokenKind::SelfKw | TokenKind::SuperKw
            ) {
                return None;
            }
            kind = self.nth_nontrivia_kind_from(&mut cursor)?;
            while kind == TokenKind::ColonColon {
                kind = self.nth_nontrivia_kind_from(&mut cursor)?;
                if !matches!(
                    kind,
                    TokenKind::Ident | TokenKind::CrateKw | TokenKind::SelfKw | TokenKind::SuperKw
                ) {
                    return None;
                }
                kind = self.nth_nontrivia_kind_from(&mut cursor)?;
            }
            if kind == TokenKind::Eq {
                kind = self.nth_nontrivia_kind_from(&mut cursor)?;
                while kind != TokenKind::RBracket {
                    if kind == TokenKind::Eof {
                        return None;
                    }
                    kind = self.nth_nontrivia_kind_from(&mut cursor)?;
                }
            } else if kind == TokenKind::LParen {
                let mut depth = 1usize;
                while depth > 0 {
                    kind = self.nth_nontrivia_kind_from(&mut cursor)?;
                    match kind {
                        TokenKind::LParen => depth += 1,
                        TokenKind::RParen => depth -= 1,
                        TokenKind::Eof => return None,
                        _ => {}
                    }
                }
                kind = self.nth_nontrivia_kind_from(&mut cursor)?;
            }
            if kind != TokenKind::RBracket {
                return None;
            }
            kind = self.nth_nontrivia_kind_from(&mut cursor)?;
        }
        if kind == TokenKind::PubKw {
            let next = self.nth_nontrivia_kind_from(&mut cursor)?;
            if next == TokenKind::LParen {
                self.nth_nontrivia_kind_from(&mut cursor)?;
                self.nth_nontrivia_kind_from(&mut cursor)?;
                self.nth_nontrivia_kind_from(&mut cursor)
            } else {
                Some(next)
            }
        } else {
            Some(kind)
        }
    }

    pub(crate) fn parse_attributes(&mut self) -> Attributes {
        let mut attributes = Vec::new();
        while self.at(TokenKind::Hash) && self.continue_parsing() {
            let mark = self.begin();
            let hash = self.take();
            let open = self.expect(TokenKind::LBracket, DiagnosticKind::UnexpectedToken);
            let path = self.parse_path();
            let payload = if let Some(equal) = self.eat(TokenKind::Eq) {
                Some(AttributePayload::Value {
                    equal,
                    value: self.parse_attribute_value().node,
                })
            } else if self.at(TokenKind::LParen) {
                let args_mark = self.begin();
                let open = self.take();
                let (arguments, _) = self.list(
                    TokenKind::RParen,
                    Self::attribute_value_starts,
                    Self::parse_attribute_arg,
                    1,
                    0,
                );
                let close = self.expect(TokenKind::RParen, DiagnosticKind::ExpectedClosingParen);
                let site = self.end(args_mark);
                Some(AttributePayload::Arguments(AttributeArgs {
                    site,
                    open,
                    arguments,
                    close,
                }))
            } else {
                None
            };
            let close = self.expect(TokenKind::RBracket, DiagnosticKind::UnexpectedToken);
            let site = self.end(mark);
            attributes.push(Attribute {
                site,
                hash,
                open,
                path,
                payload,
                close,
            });
        }
        Attributes::new(attributes)
    }

    fn attribute_value_starts(&self) -> bool {
        self.path_starts()
            || self.at_any(&[
                TokenKind::Number,
                TokenKind::Float,
                TokenKind::String,
                TokenKind::Char,
                TokenKind::ByteChar,
                TokenKind::TrueKw,
                TokenKind::FalseKw,
                TokenKind::LBracket,
            ])
    }

    fn parse_attribute_arg(&mut self) -> Built<AttributeArg> {
        let mark = self.begin();
        let key = if self.at(TokenKind::Ident) && self.nth_nontrivia_kind(1) == Some(TokenKind::Eq)
        {
            let Required::Present(name) = self.parse_name(DiagnosticKind::ExpectedFunctionName)
            else {
                unreachable!("looked-ahead name")
            };
            Some(AttributeKey {
                name,
                equal: self.take(),
            })
        } else {
            None
        };
        let value = self.parse_attribute_value();
        let site = self.end(mark);
        Built::present(
            AttributeArg {
                site,
                key,
                value: value.node,
            },
            self.completed_depth,
        )
    }

    fn parse_attribute_value(&mut self) -> Built<AttributeValue> {
        self.with_nesting(Self::parse_attribute_value_inner)
            .unwrap_or_else(|| Built::missing(self.anchor()))
    }

    fn parse_attribute_value_inner(&mut self) -> Built<AttributeValue> {
        match self.current_kind() {
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
                Built::present(AttributeValue::Literal(literal), self.completed_depth)
            }
            TokenKind::LBracket => {
                let mark = self.begin();
                let open = self.take();
                let (elements, _) = self.list(
                    TokenKind::RBracket,
                    Self::attribute_value_starts,
                    Self::parse_attribute_arg,
                    1,
                    0,
                );
                let close =
                    self.expect(TokenKind::RBracket, DiagnosticKind::ExpectedClosingBracket);
                let site = self.end(mark);
                Built::present(
                    AttributeValue::List(AttributeList {
                        site,
                        open,
                        elements,
                        close,
                    }),
                    self.completed_depth,
                )
            }
            _ if self.path_starts() => {
                let mark = self.begin();
                let path = self.parse_path();
                self.end(mark);
                Built {
                    node: match path {
                        Required::Present(path) => Required::Present(AttributeValue::Path(path)),
                        Required::Missing { at } => Required::Missing { at },
                    },
                    depth: self.completed_depth,
                }
            }
            _ => {
                self.error_here(DiagnosticKind::ExpectedExpression);
                Built::missing(self.anchor())
            }
        }
    }
}
