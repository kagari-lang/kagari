use kagari_common::{Diagnostic, SourceFile};

use crate::{
    ast::{Function, Item, Module, Parameter, TypeRef},
    lexer::lex,
    token::{Token, TokenKind},
};

pub fn parse_module(source: &SourceFile) -> Result<Module, Vec<Diagnostic>> {
    let tokens = lex(source.text());
    let mut parser = Parser::new(tokens);
    parser.parse_module()
}

struct Parser {
    tokens: Vec<Token>,
    cursor: usize,
    diagnostics: Vec<Diagnostic>,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            cursor: 0,
            diagnostics: Vec::new(),
        }
    }

    fn parse_module(&mut self) -> Result<Module, Vec<Diagnostic>> {
        let mut items = Vec::new();

        while !self.at_eof() {
            match self.peek_kind() {
                TokenKind::Fn => {
                    if let Some(function) = self.parse_function() {
                        items.push(Item::Function(function));
                    }
                }
                TokenKind::Unknown(ch) => {
                    self.diagnostics.push(
                        Diagnostic::error(format!("unexpected character `{ch}`"))
                            .with_span(self.peek().span),
                    );
                    self.bump();
                }
                _ => {
                    self.diagnostics.push(
                        Diagnostic::error("expected a top-level `fn` item")
                            .with_span(self.peek().span),
                    );
                    self.bump();
                }
            }
        }

        if self.diagnostics.is_empty() {
            Ok(Module { items })
        } else {
            Err(std::mem::take(&mut self.diagnostics))
        }
    }

    fn parse_function(&mut self) -> Option<Function> {
        self.expect_fn();
        let name = self.expect_ident("expected function name")?;
        self.expect_punct(TokenKind::LParen, "expected `(` after function name")?;

        let mut params = Vec::new();
        while !matches!(self.peek_kind(), TokenKind::RParen | TokenKind::Eof) {
            let param_name = self.expect_ident("expected parameter name")?;
            self.expect_punct(TokenKind::Colon, "expected `:` after parameter name")?;
            let ty = self.expect_ident("expected parameter type")?;
            params.push(Parameter {
                name: param_name,
                ty: TypeRef { name: ty },
            });

            if matches!(self.peek_kind(), TokenKind::Comma) {
                self.bump();
            } else {
                break;
            }
        }

        self.expect_punct(TokenKind::RParen, "expected `)` after parameters")?;

        let return_type = if matches!(self.peek_kind(), TokenKind::Arrow) {
            self.bump();
            Some(TypeRef {
                name: self.expect_ident("expected return type after `->`")?,
            })
        } else {
            None
        };

        self.expect_punct(TokenKind::LBrace, "expected `{` to start function body")?;
        self.skip_block();

        Some(Function {
            name,
            params,
            return_type,
        })
    }

    fn skip_block(&mut self) {
        let mut depth = 1usize;
        while depth > 0 && !self.at_eof() {
            match self.peek_kind() {
                TokenKind::LBrace => {
                    depth += 1;
                    self.bump();
                }
                TokenKind::RBrace => {
                    depth -= 1;
                    self.bump();
                }
                _ => {
                    self.bump();
                }
            }
        }
    }

    fn expect_fn(&mut self) {
        if matches!(self.peek_kind(), TokenKind::Fn) {
            self.bump();
        }
    }

    fn expect_ident(&mut self, message: &str) -> Option<String> {
        match self.bump().kind {
            TokenKind::Ident(name) => Some(name),
            _ => {
                self.diagnostics.push(Diagnostic::error(message));
                None
            }
        }
    }

    fn expect_punct(&mut self, expected: TokenKind, message: &str) -> Option<()> {
        if std::mem::discriminant(self.peek_kind()) == std::mem::discriminant(&expected) {
            self.bump();
            Some(())
        } else {
            self.diagnostics
                .push(Diagnostic::error(message).with_span(self.peek().span));
            None
        }
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.cursor]
    }

    fn peek_kind(&self) -> &TokenKind {
        &self.peek().kind
    }

    fn bump(&mut self) -> Token {
        let token = self.tokens[self.cursor].clone();
        self.cursor += 1;
        token
    }

    fn at_eof(&self) -> bool {
        matches!(self.peek_kind(), TokenKind::Eof)
    }
}
