//! Trait/implementation members preserve attributes, declaration bodies and written visibility.

use kagari_source::diagnostic::DiagnosticKind;

use crate::{
    ast::item::{
        AssociatedDefault, AssociatedType, FunctionBody, ImplBlock, ImplTrait, Member, MethodDef,
        Supertraits, TraitDef,
    },
    parser::{ParseMode, core::Parser},
    token::TokenKind,
};

impl Parser<'_> {
    pub(crate) fn parse_trait(&mut self) -> TraitDef {
        let mark = self.begin();
        let attributes = self.parse_attributes();
        let visibility = self.parse_visibility();
        let keyword = self.expect(TokenKind::TraitKw, DiagnosticKind::ExpectedTraitKeyword);
        let name = self.parse_name(DiagnosticKind::ExpectedTraitName);
        let generics = self.parse_generic_params();
        let supertraits = self.eat(TokenKind::Colon).map(|colon| Supertraits {
            colon,
            bounds: self.parse_trait_bound_list().node,
        });
        let open = self.expect(TokenKind::LBrace, DiagnosticKind::ExpectedStructBodyStart);
        let members = self.parse_members(false);
        let close = self.expect(TokenKind::RBrace, DiagnosticKind::ExpectedBlockEnd);
        let site = self.end(mark);
        TraitDef {
            site,
            attributes,
            visibility,
            keyword,
            name,
            generics,
            supertraits,
            open,
            members,
            close,
        }
    }

    pub(crate) fn parse_impl(&mut self) -> ImplBlock {
        let mark = self.begin();
        let attributes = self.parse_attributes();
        let keyword = self.take();
        let generics = self.parse_generic_params();
        let trait_clause = self.impl_target_followed_by_for().then(|| {
            let trait_ref = self.parse_trait_ref().node;
            let for_keyword = self.expect(TokenKind::ForKw, DiagnosticKind::ExpectedForKeyword);
            ImplTrait {
                trait_ref,
                for_keyword,
            }
        });
        let target = self.parse_type_ref().node;
        let where_clause = self.parse_where_clause();
        let open = self.expect(TokenKind::LBrace, DiagnosticKind::ExpectedStructBodyStart);
        let members = self.parse_members(true);
        let close = self.expect(TokenKind::RBrace, DiagnosticKind::ExpectedBlockEnd);
        let site = self.end(mark);
        ImplBlock {
            site,
            attributes,
            keyword,
            generics,
            trait_clause,
            target,
            where_clause,
            open,
            members,
            close,
        }
    }

    fn parse_members(&mut self, implementation: bool) -> Vec<Member> {
        let mut members = Vec::new();
        while !self.at_any(&[TokenKind::RBrace, TokenKind::Eof]) {
            self.position(6, members.len());
            let before = self.cursor();
            let member = match self.attributed_item_kind() {
                Some(TokenKind::TypeKw) => {
                    Member::AssociatedType(Box::new(self.parse_associated_type()))
                }
                Some(TokenKind::ConstKw) => Member::ConstDef(Box::new(self.parse_const(true))),
                Some(TokenKind::StaticKw) => {
                    self.error_here(DiagnosticKind::UnexpectedToken);
                    Member::Error(self.parse_static().site)
                }
                Some(TokenKind::AsyncKw | TokenKind::FnKw) => {
                    Member::MethodDef(Box::new(self.parse_method(implementation)))
                }
                _ => {
                    self.error_here(DiagnosticKind::ExpectedFunctionKeyword);
                    self.bump_as_error();
                    if self.cursor() == before {
                        break;
                    }
                    continue;
                }
            };
            members.push(member);
            if self.cursor() == before && !self.at(TokenKind::Eof) {
                self.bump_as_error();
            }
        }
        members.shrink_to_fit();
        members
    }

    fn parse_method(&mut self, allow_visibility: bool) -> MethodDef {
        let mark = self.begin();
        let attributes = self.parse_attributes();
        if !allow_visibility && self.at(TokenKind::PubKw) {
            self.error_here(DiagnosticKind::UnexpectedToken);
        }
        let visibility = self.parse_visibility();
        let async_keyword = self.eat(TokenKind::AsyncKw);
        let keyword = self.expect(TokenKind::FnKw, DiagnosticKind::ExpectedFunctionKeyword);
        let name = self.parse_name(DiagnosticKind::ExpectedFunctionName);
        let generics = self.parse_generic_params();
        let (parameters, _) = self.parse_parameters(true);
        let result = self.parse_return_type();
        let where_clause = self.parse_where_clause();
        let body = if self.at(TokenKind::LBrace) {
            FunctionBody::Block(self.parse_block().node.map(Box::new))
        } else {
            FunctionBody::Declaration(
                self.expect(TokenKind::Semi, DiagnosticKind::ExpectedStatementTerminator),
            )
        };
        let site = self.end(mark);
        MethodDef {
            site,
            attributes,
            visibility,
            async_keyword,
            keyword,
            name,
            generics,
            parameters,
            result,
            where_clause,
            body,
        }
    }

    fn parse_associated_type(&mut self) -> AssociatedType {
        let mark = self.begin();
        let attributes = self.parse_attributes();
        if self.mode != ParseMode::Declarations && self.at(TokenKind::PubKw) {
            self.error_here(DiagnosticKind::UnexpectedToken);
        }
        let visibility = self.parse_visibility();
        let keyword = self.take();
        let name = self.parse_name(DiagnosticKind::ExpectedFunctionName);
        let generics = self.parse_generic_params();
        let bounds = self.eat(TokenKind::Colon).map(|colon| Supertraits {
            colon,
            bounds: self.parse_trait_bound_list().node,
        });
        let default = self.eat(TokenKind::Eq).map(|equal| AssociatedDefault {
            equal,
            ty: self.parse_type_ref().node,
        });
        let where_clause = self.parse_where_clause();
        let terminator = self.expect(TokenKind::Semi, DiagnosticKind::ExpectedStatementTerminator);
        let site = self.end(mark);
        AssociatedType {
            site,
            attributes,
            visibility,
            keyword,
            name,
            generics,
            bounds,
            default,
            where_clause,
            terminator,
        }
    }
}
