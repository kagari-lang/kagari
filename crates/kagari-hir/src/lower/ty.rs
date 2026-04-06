use kagari_syntax::ast;
use smallvec::SmallVec;

use crate::hir::{TypeData, TypeKind};
use crate::lower::context::{Lowerer, syntax_span};

impl Lowerer {
    pub(crate) fn lower_type(&mut self, ty: &ast::TypeRef) -> crate::hir::TypeRefId {
        let kind = if let Some(name) = ty.name_text() {
            TypeKind::Named(name)
        } else if let Some(tuple) = ty.tuple_type() {
            TypeKind::Tuple(
                tuple
                    .element_types()
                    .map(|element| self.lower_type(&element))
                    .collect::<SmallVec<[_; 4]>>(),
            )
        } else if let Some(array) = ty.array_type() {
            TypeKind::Array(
                array
                    .element_type()
                    .map(|element| self.lower_type(&element))
                    .unwrap_or_else(|| self.synthetic_named_type("<missing>")),
            )
        } else {
            TypeKind::Named("<missing>".to_string())
        };

        self.alloc_type(syntax_span(ty), TypeData { kind })
    }
}
