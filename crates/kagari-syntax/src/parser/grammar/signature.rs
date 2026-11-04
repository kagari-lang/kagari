//! Name/path admission, signature punctuation, generics and trait bounds.

use kagari_source::diagnostic::DiagnosticKind;

use crate::{
    ast::{
        item::{Visibility, VisibilityRestriction},
        misc::{
            BoundClause, CallableTrait, GenericParam, GenericParamList, Name, Param, ParamList,
            ParameterType, Path, PathSegment, PathSegments, ReturnType, TraitApplication,
            TraitBound, TraitBoundList, TraitRef, TypeAnnotation, WhereClause, WherePredicate,
        },
        site::{ListEntry, Required},
    },
    parser::core::{Built, Parser},
    token::TokenKind,
};

impl Parser<'_> {
    pub(crate) fn parse_name(&mut self, diagnostic: DiagnosticKind) -> Required<Name> {
        if self.at(TokenKind::Ident) {
            {
                let name = Name { token: self.take() };
                self.account_depth(1);
                Required::Present(name)
            }
        } else {
            self.error_here(diagnostic);
            Required::Missing { at: self.anchor() }
        }
    }

    pub(crate) fn path_starts(&self) -> bool {
        self.at_any(&[
            TokenKind::Ident,
            TokenKind::CrateKw,
            TokenKind::SelfKw,
            TokenKind::SuperKw,
        ])
    }

    pub(crate) fn parse_path(&mut self) -> Required<Path> {
        if !self.path_starts() {
            self.error_here(DiagnosticKind::ExpectedPath);
            return Required::Missing { at: self.anchor() };
        }
        let mark = self.begin();
        let first = Name { token: self.take() };
        self.account_depth(1);
        let mut rest = Vec::new();
        while self.at(TokenKind::ColonColon)
            && matches!(
                self.nth_nontrivia_kind(1),
                Some(
                    TokenKind::Ident | TokenKind::CrateKw | TokenKind::SelfKw | TokenKind::SuperKw
                )
            )
        {
            let separator = self.take();
            let name = {
                let name = Name { token: self.take() };
                self.account_depth(1);
                Required::Present(name)
            };
            rest.push(PathSegment { separator, name });
        }
        let site = self.end(mark);
        Required::Present(if rest.is_empty() {
            Path::Single(first)
        } else {
            rest.shrink_to_fit();
            Path::Segmented(Box::new(PathSegments { site, first, rest }))
        })
    }

    pub(crate) fn skip_angle_group(&self, cursor: &mut usize) -> bool {
        let mut depth = 1;
        while let Some(kind) = self.nth_nontrivia_kind_from(cursor) {
            match kind {
                TokenKind::Lt => depth += 1,
                TokenKind::Gt => {
                    depth -= 1;
                    if depth == 0 {
                        return true;
                    }
                }
                TokenKind::Eof => return false,
                _ => {}
            }
        }
        false
    }

    pub(crate) fn impl_target_followed_by_for(&self) -> bool {
        let mut cursor = self.cursor();
        if !matches!(
            self.nth_nontrivia_kind_from(&mut cursor),
            Some(TokenKind::Ident | TokenKind::CrateKw | TokenKind::SelfKw | TokenKind::SuperKw)
        ) {
            return false;
        }
        loop {
            match self.nth_nontrivia_kind_from(&mut cursor) {
                Some(TokenKind::ColonColon) => {
                    if !matches!(
                        self.nth_nontrivia_kind_from(&mut cursor),
                        Some(
                            TokenKind::Ident
                                | TokenKind::CrateKw
                                | TokenKind::SelfKw
                                | TokenKind::SuperKw
                        )
                    ) {
                        return false;
                    }
                }
                Some(TokenKind::Lt) => {
                    if !self.skip_angle_group(&mut cursor) {
                        return false;
                    }
                }
                Some(TokenKind::ForKw) => return true,
                _ => return false,
            }
        }
    }

    pub(crate) fn parse_visibility(&mut self) -> Visibility {
        let Some(keyword) = self.eat(TokenKind::PubKw) else {
            return Visibility::Private;
        };
        let restriction = self.eat(TokenKind::LParen).map(|open| {
            let scope = if self.at_any(&[TokenKind::CrateKw, TokenKind::SuperKw]) {
                Required::Present(self.take())
            } else {
                self.error_here(DiagnosticKind::UnexpectedToken);
                Required::Missing { at: self.anchor() }
            };
            let close = self.expect(TokenKind::RParen, DiagnosticKind::ExpectedClosingParen);
            Box::new(VisibilityRestriction { open, scope, close })
        });
        Visibility::Public {
            keyword,
            restriction,
        }
    }

    pub(crate) fn parse_annotation(&mut self) -> Option<Box<TypeAnnotation>> {
        self.eat(TokenKind::Colon).map(|colon| {
            Box::new(TypeAnnotation {
                colon: Required::Present(colon),
                ty: self.parse_type_ref().node,
            })
        })
    }

    pub(crate) fn parse_return_type(&mut self) -> Option<ReturnType> {
        self.eat(TokenKind::Arrow).map(|arrow| ReturnType {
            arrow,
            ty: self.parse_type_ref().node,
        })
    }

    fn generic_param_starts(&self) -> bool {
        self.at(TokenKind::Ident)
    }

    fn parse_generic_param(&mut self) -> Built<GenericParam> {
        let mark = self.begin();
        let name = self.parse_name(DiagnosticKind::ExpectedGenericParameterName);
        let bounds = self.eat(TokenKind::Colon).map(|colon| {
            Box::new(BoundClause {
                colon,
                bounds: self.parse_trait_bound_list().node,
            })
        });
        let site = self.end(mark);
        Built::present(GenericParam { site, name, bounds }, self.completed_depth)
    }

    pub(crate) fn parse_generic_params(&mut self) -> Option<Box<GenericParamList>> {
        if !self.at(TokenKind::Lt) {
            return None;
        }
        let mark = self.begin();
        let open = Required::Present(self.take());
        let (parameters, _) = self.list(
            TokenKind::Gt,
            Self::generic_param_starts,
            Self::parse_generic_param,
            1,
        );
        let close = self.expect(TokenKind::Gt, DiagnosticKind::ExpectedGenericParameterName);
        let site = self.end(mark);
        Some(Box::new(GenericParamList {
            site,
            open,
            parameters,
            close,
        }))
    }

    pub(crate) fn parse_trait_bound_list(&mut self) -> Built<TraitBoundList> {
        let mark = self.begin();
        let first = self.parse_trait_ref();
        let mut rest = Vec::new();
        while let Some(plus) = self.eat(TokenKind::Plus) {
            let trait_ref = self.parse_trait_ref();
            rest.push(TraitBound {
                plus,
                trait_ref: trait_ref.node,
            });
        }
        rest.shrink_to_fit();
        let site = self.end(mark);
        Built::present(
            TraitBoundList {
                site,
                first: first.node,
                rest,
            },
            self.completed_depth,
        )
    }

    pub(crate) fn parse_trait_ref(&mut self) -> Built<TraitRef> {
        let mark = self.begin();
        let path = self.parse_path();
        let application = if self.at(TokenKind::Lt) {
            let (arguments, _) = self.parse_generic_arg_list();
            Some(Box::new(TraitApplication::Generic(arguments)))
        } else if self.at(TokenKind::LParen) {
            let callable_mark = self.begin();
            let open = self.take();
            let (inputs, _) = self.list(
                TokenKind::RParen,
                Self::type_starts,
                Self::parse_type_ref,
                1,
            );
            let close = self.expect(TokenKind::RParen, DiagnosticKind::ExpectedClosingParen);
            let output = self.parse_return_type();
            let site = self.end(callable_mark);
            Some(Box::new(TraitApplication::Callable(CallableTrait {
                site,
                open,
                inputs,
                close,
                output,
            })))
        } else {
            None
        };
        let site = self.end(mark);
        Built::present(
            TraitRef {
                site,
                path,
                application,
            },
            self.completed_depth,
        )
    }

    fn parse_param(&mut self) -> Built<Param> {
        let mark = self.begin();
        let name = self.parse_name(DiagnosticKind::ExpectedParameterName);
        let colon = self.expect(
            TokenKind::Colon,
            DiagnosticKind::ExpectedParameterTypeSeparator,
        );
        let ty = self.parse_type_ref();
        let site = self.end(mark);
        Built::present(
            Param {
                site,
                name,
                annotation: ParameterType::Explicit { colon, ty: ty.node },
            },
            self.completed_depth,
        )
    }

    pub(crate) fn parse_parameters(&mut self, method: bool) -> (ParamList, usize) {
        let mark = self.begin();
        let open = self.expect(
            TokenKind::LParen,
            DiagnosticKind::ExpectedFunctionParameterListStart,
        );
        let mut parameters = Vec::new();
        if method && self.at(TokenKind::SelfKw) {
            let receiver_mark = self.begin();
            let name = {
                let name = Name { token: self.take() };
                self.account_depth(1);
                Required::Present(name)
            };
            let site = self.end(receiver_mark);
            let separator = self.eat(TokenKind::Comma).map(Required::Present);
            let more = separator.is_some();
            parameters.push(ListEntry {
                element: Required::Present(Param {
                    site,
                    name,
                    annotation: ParameterType::Receiver,
                }),
                separator,
            });
            if more {
                let (rest, _) = self.list(
                    TokenKind::RParen,
                    Self::generic_param_starts,
                    Self::parse_param,
                    1,
                );
                parameters.extend(rest.entries);
            }
        } else {
            let (parsed, _) = self.list(
                TokenKind::RParen,
                Self::generic_param_starts,
                Self::parse_param,
                1,
            );
            parameters = parsed.entries.into_vec();
        }

        let close = self.expect(
            TokenKind::RParen,
            DiagnosticKind::ExpectedFunctionParameterListEnd,
        );
        let site = self.end(mark);
        (
            ParamList {
                site,
                open,
                parameters: parameters.into(),
                close,
            },
            self.completed_depth,
        )
    }

    fn parse_where_predicate(&mut self) -> Built<WherePredicate> {
        let mark = self.begin();
        let target = self.parse_type_ref();
        let colon = self.expect(
            TokenKind::Colon,
            DiagnosticKind::ExpectedWherePredicateSeparator,
        );
        let bounds = self.parse_trait_bound_list();
        let site = self.end(mark);
        Built::present(
            WherePredicate {
                site,
                target: target.node,
                colon,
                bounds: bounds.node,
            },
            self.completed_depth,
        )
    }

    pub(crate) fn parse_where_clause(&mut self) -> Option<Box<WhereClause>> {
        if !self.at(TokenKind::WhereKw) {
            return None;
        }
        let mark = self.begin();
        let keyword = self.take();
        let mut predicates = Vec::new();
        while !self.at_any(&[
            TokenKind::LBrace,
            TokenKind::RBrace,
            TokenKind::Eq,
            TokenKind::Semi,
            TokenKind::Eof,
        ]) {
            let predicate = self.parse_where_predicate();
            let separator = self.eat(TokenKind::Comma).map(Required::Present);
            let more = separator.is_some();
            predicates.push(ListEntry {
                element: predicate.node,
                separator,
            });
            if !more {
                break;
            }
        }

        let site = self.end(mark);
        Some(Box::new(WhereClause {
            site,
            keyword,
            predicates: predicates.into(),
        }))
    }
}
