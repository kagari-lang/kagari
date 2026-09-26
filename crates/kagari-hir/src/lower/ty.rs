use kagari_syntax::ast;
use smallvec::SmallVec;

use crate::hir::{TypeData, TypeKind, TypeRefId};
use crate::lower::context::{Lowerer, syntax_span, token_span};

impl Lowerer {
    pub(crate) fn lower_type(&mut self, ty: &ast::TypeRef) -> TypeRefId {
        if let Some(inner) = ty.grouped_type() {
            return self.lower_type(&inner);
        }
        let kind = if let Some(qualified) = ty.qualified_type() {
            TypeKind::Projection {
                arguments: qualified
                    .generic_args()
                    .map(|args| args.args().map(|arg| self.lower_type(&arg)).collect())
                    .unwrap_or_default(),
                receiver: qualified
                    .receiver()
                    .map(|ty| self.lower_type(&ty))
                    .unwrap_or_else(|| self.synthetic_named_type("<missing>")),
                trait_ref: qualified
                    .trait_ref()
                    .map(|ty| self.lower_trait_ref(&ty).ty)
                    .unwrap_or_else(|| self.synthetic_named_type("<missing>")),
                member: qualified
                    .member()
                    .and_then(|name| name.text())
                    .unwrap_or_default(),
            }
        } else if let Some(name) = ty.name_text() {
            let args = ty
                .generic_args()
                .map(|args| {
                    args.args()
                        .map(|arg| self.lower_type(&arg))
                        .collect::<SmallVec<[_; 4]>>()
                })
                .unwrap_or_default();
            if ty.generic_args().is_none() {
                TypeKind::Named(name)
            } else {
                let list = ty.generic_args().expect("generic argument list");
                let bindings = self.lower_associated_bindings(&list);
                TypeKind::Generic {
                    name,
                    args,
                    bindings,
                    positional_after_binding: list.positional_after_binding(),
                }
            }
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
        } else if let Some(function) = ty.function_type() {
            TypeKind::Function {
                params: function
                    .params()
                    .map(|param| self.lower_type(&param))
                    .collect(),
                result: function
                    .result()
                    .map(|result| self.lower_type(&result))
                    .unwrap_or_else(|| self.synthetic_named_type("<missing>")),
            }
        } else {
            TypeKind::Named("<missing>".to_string())
        };

        let id = self.alloc_type(syntax_span(ty), TypeData { kind });
        if let Some(name) = ty
            .path()
            .and_then(|path| path.segments().last())
            .or_else(|| ty.name())
        {
            self.source_map.insert_type_name(id, token_span(&name));
        }
        id
    }

    pub(crate) fn lower_associated_bindings(
        &mut self,
        list: &ast::GenericArgList,
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
