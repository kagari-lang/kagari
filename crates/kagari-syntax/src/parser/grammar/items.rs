//! Physical modules, declaration headers, imports and ordinary data declarations.

use kagari_source::diagnostic::DiagnosticKind;

use crate::{
    ast::{
        item::{
            AliasTarget, ConstDef, ConstInitializer, EnumDef, FnDef, FunctionBody, InlineModule,
            Item, ModuleBody, ModuleDef, ModuleForm, ModuleId, OpaqueTypeDef, StaticDef, StructDef,
            Supertraits, TypeAlias, UseDecl, UseSuffix, UseTree, UseTreeList,
        },
        misc::{Field, FieldList, TuplePayload, TypeAnnotation, Variant, VariantList},
        site::Required,
        tokens,
    },
    parser::{
        ParseMode,
        core::{Built, Parser},
    },
    token::TokenKind,
};

impl Parser<'_> {
    pub(crate) fn parse_root(&mut self) -> ModuleId {
        self.parse_module_body(false)
    }

    fn parse_module_body(&mut self, inline: bool) -> ModuleId {
        let id = ModuleId(
            self.modules
                .len()
                .try_into()
                .expect("parse-local module ID"),
        );
        self.modules.push(None);
        let mark = self.begin();
        let open = inline.then(|| self.take::<tokens::LBrace>());
        let mut items = Vec::new();
        while !self.at(TokenKind::Eof) && !(inline && self.at(TokenKind::RBrace)) {
            self.position(1, items.len());
            let before = self.cursor();
            if let Some(item) = self.parse_item() {
                items.push(item);
            }
            if self.cursor() == before && !self.at(TokenKind::Eof) {
                self.bump_as_error();
            }
        }
        let close =
            inline.then(|| self.expect(TokenKind::RBrace, DiagnosticKind::ExpectedBlockEnd));
        if !inline {
            self.finish_remaining_tokens();
        }
        let site = self.end(mark);
        items.shrink_to_fit();
        self.modules[id.0 as usize] = Some(ModuleBody {
            site,
            open,
            items,
            close,
        });
        id
    }

    fn parse_item(&mut self) -> Option<Item> {
        match self.attributed_item_kind() {
            Some(TokenKind::ModKw) => self
                .with_nesting(Self::parse_module_def)
                .map(|node| Item::ModuleDef(Box::new(node))),
            Some(TokenKind::UseKw) => Some(Item::UseDecl(Box::new(self.parse_use()))),
            Some(TokenKind::FnKw | TokenKind::AsyncKw) => match self.parse_function().node {
                Required::Present(function) => Some(Item::FnDef(Box::new(function))),
                Required::Missing { .. } => None,
            },
            Some(TokenKind::ConstKw) => Some(Item::ConstDef(Box::new(self.parse_const(false)))),
            Some(TokenKind::StaticKw) => Some(Item::StaticDef(Box::new(self.parse_static()))),
            Some(TokenKind::StructKw) => Some(Item::StructDef(Box::new(self.parse_struct()))),
            Some(TokenKind::EnumKw) => Some(Item::EnumDef(Box::new(self.parse_enum()))),
            Some(TokenKind::TraitKw) => Some(Item::TraitDef(Box::new(self.parse_trait()))),
            Some(TokenKind::ImplKw) if !self.at(TokenKind::PubKw) => {
                Some(Item::ImplBlock(Box::new(self.parse_impl())))
            }
            Some(TokenKind::TypeKw) => Some(self.parse_module_type()),
            _ => {
                self.error_here(if self.at(TokenKind::Unknown) {
                    DiagnosticKind::UnexpectedToken
                } else {
                    DiagnosticKind::ExpectedTopLevelItem
                });
                self.bump_as_error();
                None
            }
        }
    }

    fn parse_module_def(&mut self) -> ModuleDef {
        let mark = self.begin();
        let attributes = self.parse_attributes();
        let visibility = self.parse_visibility();
        let keyword = self.expect(TokenKind::ModKw, DiagnosticKind::ExpectedModuleKeyword);
        let name = self.parse_name(DiagnosticKind::ExpectedModuleName);
        let form = if let Some(semicolon) = self.eat(TokenKind::Semi) {
            ModuleForm::External(semicolon)
        } else {
            let body = if self.at(TokenKind::LBrace) {
                Required::Present(self.parse_module_body(true))
            } else {
                self.error_here(DiagnosticKind::ExpectedModuleBodyStart);
                Required::Missing { at: self.anchor() }
            };
            ModuleForm::Inline(InlineModule { body })
        };
        let site = self.end(mark);
        ModuleDef {
            site,
            attributes,
            visibility,
            keyword,
            name,
            form,
        }
    }

    pub(crate) fn parse_function(&mut self) -> Built<FnDef> {
        let mark = self.begin();
        let attributes = self.parse_attributes();
        let visibility = self.parse_visibility();
        let async_keyword = self.eat(TokenKind::AsyncKw);
        let keyword = self.expect(TokenKind::FnKw, DiagnosticKind::ExpectedFunctionKeyword);
        let name = self.parse_name(DiagnosticKind::ExpectedFunctionName);
        let generics = self.parse_generic_params();
        let (parameters, _) = self.parse_parameters(false);
        let result = self.parse_return_type();
        let where_clause = self.parse_where_clause();
        let body = if self.mode == ParseMode::Declarations && self.at(TokenKind::Semi) {
            FunctionBody::Declaration(Required::Present(self.take()))
        } else {
            let body = self.parse_block();
            FunctionBody::Block(body.node.map(Box::new))
        };
        let site = self.end(mark);
        Built::present(
            FnDef {
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
            },
            self.completed_depth,
        )
    }

    pub(crate) fn parse_const(&mut self, associated: bool) -> ConstDef {
        let mark = self.begin();
        let attributes = self.parse_attributes();
        let visibility = self.parse_visibility();
        let keyword = self.expect(TokenKind::ConstKw, DiagnosticKind::ExpectedConstKeyword);
        let name = self.parse_name(DiagnosticKind::ExpectedConstName);
        let annotation = if associated {
            let colon = self.expect(TokenKind::Colon, DiagnosticKind::ExpectedType);
            Some(Box::new(TypeAnnotation {
                colon,
                ty: self.parse_type_ref().node,
            }))
        } else {
            self.parse_annotation()
        };
        let initializer = if associated && !self.at(TokenKind::Eq) {
            ConstInitializer::Declaration
        } else {
            let equal = self.expect(TokenKind::Eq, DiagnosticKind::ExpectedConstInitializer);
            let value = self.parse_expr().node;
            ConstInitializer::Definition { equal, value }
        };
        let terminator = self.expect(TokenKind::Semi, DiagnosticKind::ExpectedStatementTerminator);
        let site = self.end(mark);
        ConstDef {
            site,
            attributes,
            visibility,
            keyword,
            name,
            annotation,
            initializer,
            terminator,
        }
    }

    pub(crate) fn parse_static(&mut self) -> StaticDef {
        let mark = self.begin();
        let attributes = self.parse_attributes();
        let visibility = self.parse_visibility();
        let keyword = self.expect(TokenKind::StaticKw, DiagnosticKind::UnexpectedToken);
        let binding = self.parse_binding_kind(DiagnosticKind::ExpectedBindingKeyword);
        let name = self.parse_name(DiagnosticKind::ExpectedBindingName);
        let colon = self.expect(TokenKind::Colon, DiagnosticKind::ExpectedType);
        let ty = if self.at(TokenKind::FnKw) && self.nth_nontrivia_kind(1) == Some(TokenKind::Ident)
        {
            self.error_here(DiagnosticKind::ExpectedType);
            Required::Missing { at: self.anchor() }
        } else {
            self.parse_type_ref().node
        };
        let equal = self.expect(TokenKind::Eq, DiagnosticKind::ExpectedAssignmentOperator);
        let initializer = self.parse_expr().node;
        let terminator = self.expect(TokenKind::Semi, DiagnosticKind::ExpectedStatementTerminator);
        if matches!(terminator, Required::Missing { .. }) {
            self.recover_static_tail();
        }
        let site = self.end(mark);
        StaticDef {
            site,
            attributes,
            visibility,
            keyword,
            binding,
            name,
            colon,
            ty,
            equal,
            initializer,
            terminator,
        }
    }

    fn recover_static_tail(&mut self) {
        let mut braces = 0usize;
        while !self.at(TokenKind::Eof) {
            if braces == 0 {
                if self.at_any(&[
                    TokenKind::RBrace,
                    TokenKind::Hash,
                    TokenKind::PubKw,
                    TokenKind::ModKw,
                    TokenKind::UseKw,
                    TokenKind::AsyncKw,
                    TokenKind::FnKw,
                    TokenKind::TypeKw,
                    TokenKind::ConstKw,
                    TokenKind::StaticKw,
                    TokenKind::StructKw,
                    TokenKind::EnumKw,
                    TokenKind::TraitKw,
                    TokenKind::ImplKw,
                ]) {
                    break;
                }
                if self.at(TokenKind::Semi) {
                    self.bump_as_error();
                    break;
                }
            }
            if self.at(TokenKind::LBrace) {
                braces += 1;
            } else if self.at(TokenKind::RBrace) {
                braces = braces.saturating_sub(1);
            }
            self.bump_as_error();
        }
    }

    fn field_starts(&self) -> bool {
        self.at_any(&[
            TokenKind::Hash,
            TokenKind::PubKw,
            TokenKind::ValKw,
            TokenKind::VarKw,
            TokenKind::Ident,
        ])
    }

    fn parse_field(&mut self) -> Built<Field> {
        let mark = self.begin();
        let attributes = self.parse_attributes();
        let visibility = self.parse_visibility();
        let binding = self.parse_binding_kind(DiagnosticKind::ExpectedFieldBinding);
        let name = self.parse_name(DiagnosticKind::ExpectedFieldName);
        let colon = self.expect(TokenKind::Colon, DiagnosticKind::ExpectedFieldTypeSeparator);
        let ty = self.parse_type_ref();
        let site = self.end(mark);
        Built::present(
            Field {
                site,
                attributes,
                visibility,
                binding,
                name,
                colon,
                ty: ty.node,
            },
            self.completed_depth,
        )
    }

    fn parse_struct(&mut self) -> StructDef {
        let mark = self.begin();
        let attributes = self.parse_attributes();
        let visibility = self.parse_visibility();
        let keyword = self.expect(TokenKind::StructKw, DiagnosticKind::ExpectedStructKeyword);
        let name = self.parse_name(DiagnosticKind::ExpectedStructName);
        let generics = self.parse_generic_params();
        let fields_mark = self.begin();
        let open = self.expect(TokenKind::LBrace, DiagnosticKind::ExpectedTraitBodyStart);
        let (fields, _) = self.list(TokenKind::RBrace, Self::field_starts, Self::parse_field, 1);
        let close = self.expect(TokenKind::RBrace, DiagnosticKind::ExpectedBlockEnd);
        let site = self.end(fields_mark);
        let fields = Required::Present(FieldList {
            site,
            open,
            fields,
            close,
        });
        let site = self.end(mark);
        StructDef {
            site,
            attributes,
            visibility,
            keyword,
            name,
            generics,
            fields,
        }
    }

    fn variant_starts(&self) -> bool {
        self.at(TokenKind::Ident)
    }

    fn parse_variant(&mut self) -> Built<Variant> {
        let mark = self.begin();
        let name = self.parse_name(DiagnosticKind::ExpectedVariantName);
        let payload = if self.at(TokenKind::LParen) {
            let payload_mark = self.begin();
            let open = self.take();
            let (types, _) = self.list(
                TokenKind::RParen,
                Self::type_starts,
                Self::parse_type_ref,
                1,
            );
            let close = self.expect(TokenKind::RParen, DiagnosticKind::ExpectedClosingParen);
            let site = self.end(payload_mark);
            Some(TuplePayload {
                site,
                open,
                types,
                close,
            })
        } else {
            None
        };
        let site = self.end(mark);
        Built::present(
            Variant {
                site,
                attributes: Default::default(),
                name,
                payload,
            },
            self.completed_depth,
        )
    }

    fn parse_enum(&mut self) -> EnumDef {
        let mark = self.begin();
        let attributes = self.parse_attributes();
        let visibility = self.parse_visibility();
        let keyword = self.expect(TokenKind::EnumKw, DiagnosticKind::ExpectedEnumKeyword);
        let name = self.parse_name(DiagnosticKind::ExpectedEnumName);
        let generics = self.parse_generic_params();
        let variants_mark = self.begin();
        let open = self.expect(TokenKind::LBrace, DiagnosticKind::ExpectedImplBodyStart);
        let (variants, _) = self.list(
            TokenKind::RBrace,
            Self::variant_starts,
            Self::parse_variant,
            1,
        );
        let close = self.expect(TokenKind::RBrace, DiagnosticKind::ExpectedBlockEnd);
        let site = self.end(variants_mark);
        let variants = Required::Present(VariantList {
            site,
            open,
            variants,
            close,
        });
        let site = self.end(mark);
        EnumDef {
            site,
            attributes,
            visibility,
            keyword,
            name,
            generics,
            variants,
        }
    }

    fn parse_module_type(&mut self) -> Item {
        let mark = self.begin();
        let attributes = self.parse_attributes();
        let visibility = self.parse_visibility();
        let keyword = self.take();
        let name = self.parse_name(DiagnosticKind::ExpectedFunctionName);
        let generics = self.parse_generic_params();
        let bounds = self.eat(TokenKind::Colon).map(|colon| {
            if self.mode != ParseMode::Declarations {
                self.error_here(DiagnosticKind::UnexpectedToken);
            }
            Supertraits {
                colon,
                bounds: self.parse_trait_bound_list().node,
            }
        });
        let where_clause = self.parse_where_clause();
        if self.at(TokenKind::Eq) || self.mode != ParseMode::Declarations {
            if bounds.is_some() && self.mode == ParseMode::Declarations {
                self.error_here(DiagnosticKind::UnexpectedToken);
            }
            let equal = self.expect(TokenKind::Eq, DiagnosticKind::ExpectedAssignmentOperator);
            let ty = if matches!(equal, Required::Present(_)) {
                self.parse_type_ref().node
            } else {
                Required::Missing { at: self.anchor() }
            };
            let target = match where_clause {
                Some(clause) => AliasTarget::LeadingWhere { clause, equal, ty },
                None => AliasTarget::TrailingWhere {
                    equal,
                    ty,
                    clause: self.parse_where_clause(),
                },
            };
            let terminator =
                self.expect(TokenKind::Semi, DiagnosticKind::ExpectedStatementTerminator);
            let site = self.end(mark);
            Item::TypeAlias(Box::new(TypeAlias {
                site,
                attributes,
                visibility,
                keyword,
                name,
                generics,
                bounds,
                target,
                terminator,
            }))
        } else {
            let terminator =
                self.expect(TokenKind::Semi, DiagnosticKind::ExpectedStatementTerminator);
            let site = self.end(mark);
            Item::OpaqueTypeDef(Box::new(OpaqueTypeDef {
                site,
                attributes,
                visibility,
                keyword,
                name,
                generics,
                bounds,
                where_clause,
                terminator,
            }))
        }
    }

    fn parse_use(&mut self) -> UseDecl {
        let mark = self.begin();
        let attributes = self.parse_attributes();
        let visibility = self.parse_visibility();
        let keyword = self.expect(TokenKind::UseKw, DiagnosticKind::ExpectedUseKeyword);
        let tree = self.parse_use_tree().node;
        let terminator = self.expect(TokenKind::Semi, DiagnosticKind::ExpectedStatementTerminator);
        let site = self.end(mark);
        UseDecl {
            site,
            attributes,
            visibility,
            keyword,
            tree,
            terminator,
        }
    }

    fn use_tree_starts(&self) -> bool {
        self.path_starts() || self.at(TokenKind::LBrace)
    }

    fn parse_use_tree(&mut self) -> Built<UseTree> {
        self.with_nesting(Self::parse_use_tree_inner)
            .unwrap_or_else(|| Built::missing(self.anchor()))
    }

    fn parse_use_tree_inner(&mut self) -> Built<UseTree> {
        let mark = self.begin();
        let prefix = if self.path_starts() {
            match self.parse_path() {
                Required::Present(path) => Some(path),
                Required::Missing { .. } => None,
            }
        } else {
            None
        };
        let suffix = if self.at(TokenKind::LBrace) {
            Some(UseSuffix::Group {
                separator: None,
                group: self.parse_use_group(),
            })
        } else if let Some(keyword) = self.eat(TokenKind::AsKw) {
            Some(UseSuffix::Alias {
                keyword,
                name: self.parse_name(DiagnosticKind::ExpectedUseAlias),
            })
        } else if let Some(separator) = self.eat(TokenKind::ColonColon) {
            if self.at(TokenKind::LBrace) {
                Some(UseSuffix::Group {
                    separator: Some(separator),
                    group: self.parse_use_group(),
                })
            } else {
                Some(UseSuffix::Glob {
                    separator,
                    star: self.expect(TokenKind::Star, DiagnosticKind::ExpectedUseTree),
                })
            }
        } else {
            None
        };
        if prefix.is_none() && suffix.is_none() {
            self.error_here(DiagnosticKind::ExpectedUseTree);
        }
        let site = self.end(mark);
        Built::present(
            UseTree {
                site,
                prefix,
                suffix,
            },
            self.completed_depth,
        )
    }

    fn parse_use_group(&mut self) -> UseTreeList {
        let mark = self.begin();
        let open = self.take();
        let (trees, _) = self.list(
            TokenKind::RBrace,
            Self::use_tree_starts,
            Self::parse_use_tree,
            1,
        );
        let close = self.expect(TokenKind::RBrace, DiagnosticKind::ExpectedBlockEnd);
        let site = self.end(mark);
        UseTreeList {
            site,
            open,
            trees,
            close,
        }
    }
}
