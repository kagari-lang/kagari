//! Structural types and ordered generic arguments; no semantic probing or type inference.

use kagari_source::diagnostic::DiagnosticKind;

use crate::{
    ast::{
        misc::{AssociatedTypeBinding, GenericArgList, GenericArgument},
        site::{ListEntry, Required},
        ty::{
            AppliedType, ArrayType, FunctionType, GroupedType, QualifiedType, TupleType, TypeSyntax,
        },
    },
    parser::core::{Built, Parser},
    token::TokenKind,
};

impl Parser<'_> {
    pub(crate) fn type_starts(&self) -> bool {
        self.path_starts()
            || self.at_any(&[
                TokenKind::Bang,
                TokenKind::LParen,
                TokenKind::LBracket,
                TokenKind::FnKw,
                TokenKind::Lt,
            ])
    }

    pub(crate) fn parse_type_ref(&mut self) -> Built<TypeSyntax> {
        self.with_nesting(Self::parse_type_inner)
            .unwrap_or_else(|| Built::missing(self.anchor()))
    }

    fn parse_type_inner(&mut self) -> Built<TypeSyntax> {
        if self.path_starts() {
            let mark = self.begin();
            let path = self.parse_path();
            if self.at(TokenKind::Lt) {
                let (arguments, _) = self.parse_generic_arg_list();
                let site = self.end(mark);
                return Built::present(
                    TypeSyntax::Applied(Box::new(AppliedType {
                        site,
                        path,
                        arguments,
                    })),
                    self.completed_depth,
                );
            }
            self.end(mark);
            return Built {
                node: match path {
                    Required::Present(path) => Required::Present(TypeSyntax::Named(path)),
                    Required::Missing { at } => Required::Missing { at },
                },
                depth: self.completed_depth,
            };
        }
        match self.current_kind() {
            TokenKind::Bang => {
                let token = self.take();
                self.account_depth(1);
                Built::present(TypeSyntax::Never(token), self.completed_depth)
            }
            TokenKind::LParen => self.parse_parenthesized_type(),
            TokenKind::LBracket => {
                let mark = self.begin();
                let open = self.take();
                let element = self.parse_type_ref();
                let close =
                    self.expect(TokenKind::RBracket, DiagnosticKind::ExpectedClosingBracket);
                let site = self.end(mark);
                Built::present(
                    TypeSyntax::Array(Box::new(ArrayType {
                        site,
                        open,
                        element: element.node,
                        close,
                    })),
                    self.completed_depth,
                )
            }
            TokenKind::FnKw => {
                let mark = self.begin();
                let keyword = self.take();
                let open = self.expect(TokenKind::LParen, DiagnosticKind::ExpectedClosingParen);
                let (parameters, _) = self.list(
                    TokenKind::RParen,
                    Self::type_starts,
                    Self::parse_type_ref,
                    2,
                );
                let close = self.expect(TokenKind::RParen, DiagnosticKind::ExpectedClosingParen);
                let arrow = self.expect(TokenKind::Arrow, DiagnosticKind::ExpectedType);
                let result = self.parse_type_ref();
                let site = self.end(mark);
                Built::present(
                    TypeSyntax::Function(Box::new(FunctionType {
                        site,
                        keyword,
                        open,
                        parameters,
                        close,
                        arrow,
                        result: result.node,
                    })),
                    self.completed_depth,
                )
            }
            TokenKind::Lt => {
                let (qualified, _) = self.parse_qualified_type();
                Built::present(
                    TypeSyntax::Qualified(Box::new(qualified)),
                    self.completed_depth,
                )
            }
            _ => {
                self.error_here(DiagnosticKind::ExpectedType);
                Built::missing(self.anchor())
            }
        }
    }

    fn parse_parenthesized_type(&mut self) -> Built<TypeSyntax> {
        let mark = self.begin();
        let open = self.take();
        let first = (!self.at(TokenKind::RParen)).then(|| self.parse_type_ref());
        match (first, self.at(TokenKind::Comma)) {
            (Some(first), false) => {
                let close = self.expect(TokenKind::RParen, DiagnosticKind::ExpectedClosingParen);
                let site = self.end(mark);
                Built::present(
                    TypeSyntax::Grouped(Box::new(GroupedType {
                        site,
                        open,
                        ty: first.node,
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
                        Self::type_starts,
                        Self::parse_type_ref,
                        1,
                    );
                    elements.extend(rest.entries);
                }
                let close = self.expect(TokenKind::RParen, DiagnosticKind::ExpectedClosingParen);
                let site = self.end(mark);
                Built::present(
                    TypeSyntax::Tuple(Box::new(TupleType {
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

    pub(crate) fn parse_qualified_type(&mut self) -> (QualifiedType, usize) {
        let mark = self.begin();
        let open = self.take();
        let receiver = self.parse_type_ref();
        let as_keyword = self.expect(TokenKind::AsKw, DiagnosticKind::ExpectedType);
        let trait_ref = self.parse_trait_ref();
        let close = self.expect(TokenKind::Gt, DiagnosticKind::ExpectedType);
        let separator = self.expect(TokenKind::ColonColon, DiagnosticKind::ExpectedType);
        let member = self.parse_name(DiagnosticKind::ExpectedFunctionName);
        let arguments = self
            .at(TokenKind::Lt)
            .then(|| self.parse_generic_arg_list());
        let site = self.end(mark);
        (
            QualifiedType {
                site,
                open,
                receiver: receiver.node,
                as_keyword,
                trait_ref: trait_ref.node,
                close,
                separator,
                member,
                arguments: arguments.map(|(arguments, _)| Box::new(arguments)),
            },
            self.completed_depth,
        )
    }

    fn generic_argument_starts(&self) -> bool {
        self.type_starts()
    }

    fn parse_generic_argument(&mut self) -> Built<GenericArgument> {
        if self.at(TokenKind::Ident) && self.nth_nontrivia_kind(1) == Some(TokenKind::Eq) {
            let mark = self.begin();
            let name = self.parse_name(DiagnosticKind::ExpectedType);
            let equal = self.expect(TokenKind::Eq, DiagnosticKind::ExpectedType);
            let ty = self.parse_type_ref();
            let site = self.end(mark);
            Built::present(
                GenericArgument::Binding(AssociatedTypeBinding {
                    site,
                    name,
                    equal,
                    ty: ty.node,
                }),
                self.completed_depth,
            )
        } else {
            let ty = self.parse_type_ref();
            Built {
                node: match ty.node {
                    Required::Present(ty) => Required::Present(GenericArgument::Type(ty)),
                    Required::Missing { at } => Required::Missing { at },
                },
                depth: ty.depth,
            }
        }
    }

    pub(crate) fn parse_generic_arg_list(&mut self) -> (GenericArgList, usize) {
        let mark = self.begin();
        let open = self.expect(TokenKind::Lt, DiagnosticKind::ExpectedType);
        let (arguments, _) = self.list(
            TokenKind::Gt,
            Self::generic_argument_starts,
            Self::parse_generic_argument,
            1,
        );
        let close = self.expect(TokenKind::Gt, DiagnosticKind::ExpectedType);
        let site = self.end(mark);
        (
            GenericArgList {
                site,
                open,
                arguments,
                close,
            },
            self.completed_depth,
        )
    }
}
