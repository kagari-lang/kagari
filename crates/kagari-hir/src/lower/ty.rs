//! Exhaustive typed-syntax lowering; source maps retain whole-node origins.

use kagari_common::span::Span;
use kagari_syntax::ast::{
    borrow::Ast,
    misc::{GenericArgList, TuplePayload},
    site::Required,
    ty::{QualifiedType, TypeSyntax},
    views::TypeView,
};
use smallvec::SmallVec;

use crate::{
    hir::{ids::TypeRefId, ty::TypeKind},
    lower::context::{Lowerer, syntax_span},
};

impl Lowerer {
    pub(crate) fn lower_payload_types(
        &mut self,
        payload: &Ast<'_, TuplePayload>,
    ) -> Vec<TypeRefId> {
        payload
            .type_entries()
            .map(|entry| match entry {
                Required::Present(ty) => self.lower_type(&ty),
                Required::Missing { at } => self.alloc_type(
                    Span::new(at as usize, at as usize),
                    TypeKind::Named("<missing>".into()),
                ),
            })
            .collect()
    }

    pub(crate) fn lower_type(&mut self, ty: &Ast<'_, TypeSyntax>) -> TypeRefId {
        let kind = match ty.view() {
            TypeView::Grouped(grouped) => {
                return grouped
                    .ty()
                    .map(|inner| self.lower_type(&inner))
                    .unwrap_or_else(|| {
                        self.alloc_type(syntax_span(ty), TypeKind::Named("<missing>".into()))
                    });
            }
            TypeView::Named(path) => TypeKind::Named(path.text().unwrap_or_default()),
            TypeView::Applied(applied) => {
                let arguments = applied.arguments();
                TypeKind::Generic {
                    name: applied
                        .path()
                        .and_then(|path| path.text())
                        .unwrap_or_default(),
                    args: arguments.args().map(|ty| self.lower_type(&ty)).collect(),
                    bindings: self.lower_associated_bindings(&arguments),
                    callable_syntax: false,
                    positional_after_binding: arguments.positional_after_binding(),
                }
            }
            TypeView::Never(_) => TypeKind::Named("!".into()),
            TypeView::Tuple(tuple) => TypeKind::Tuple(
                tuple
                    .element_types()
                    .map(|ty| self.lower_type(&ty))
                    .collect::<SmallVec<[_; 4]>>(),
            ),
            TypeView::Array(array) => TypeKind::Array(
                array
                    .element_type()
                    .map(|ty| self.lower_type(&ty))
                    .unwrap_or_else(|| self.synthetic_named_type("<missing>")),
            ),
            TypeView::Function(function) => TypeKind::Function {
                params: function.params().map(|ty| self.lower_type(&ty)).collect(),
                result: function
                    .result()
                    .map(|ty| self.lower_type(&ty))
                    .unwrap_or_else(|| self.synthetic_named_type("<missing>")),
            },
            TypeView::Qualified(qualified) => self.projection_kind(&qualified),
            TypeView::Error(_) => TypeKind::Named("<missing>".into()),
        };
        self.alloc_type(syntax_span(ty), kind)
    }

    pub(crate) fn lower_qualified_type(&mut self, qualified: &Ast<'_, QualifiedType>) -> TypeRefId {
        let kind = self.projection_kind(qualified);
        self.alloc_type(syntax_span(qualified), kind)
    }

    fn projection_kind(&mut self, qualified: &Ast<'_, QualifiedType>) -> TypeKind {
        TypeKind::Projection {
            arguments: qualified
                .generic_args()
                .map(|arguments| arguments.args().map(|ty| self.lower_type(&ty)).collect())
                .unwrap_or_default(),
            receiver: qualified
                .receiver()
                .map(|ty| self.lower_type(&ty))
                .unwrap_or_else(|| self.synthetic_named_type("<missing>")),
            trait_ref: qualified
                .trait_ref()
                .map(|reference| self.lower_trait_ref(&reference).ty)
                .unwrap_or_else(|| self.synthetic_named_type("<missing>")),
            member: qualified
                .member()
                .and_then(|name| name.text())
                .unwrap_or_default(),
        }
    }

    pub(crate) fn lower_associated_bindings(
        &mut self,
        list: &Ast<'_, GenericArgList>,
    ) -> Vec<(String, TypeRefId)> {
        list.bindings()
            .map(|binding| {
                let name = binding.name_text().unwrap_or_default();
                let ty = binding
                    .ty()
                    .map(|ty| self.lower_type(&ty))
                    .unwrap_or_else(|| self.synthetic_named_type("<missing>"));
                (name, ty)
            })
            .collect()
    }
}
