//! Unresolved type construction and separate path/name/terminal navigation ranges.

use kagari_syntax::ast::{misc::GenericArgList, ty::TypeRef};

use smallvec::SmallVec;

use crate::{
    hir::{
        ids::TypeRefId,
        ty::{TypeData, TypeKind},
    },
    lower::context::{Lowerer, syntax_span, token_span},
};

impl Lowerer {
    /// Lowers type applications/projections and source path sites, collapsing grouping and retaining missing-type placeholders.
    pub(crate) fn lower_type(&mut self, ty: &TypeRef) -> TypeRefId {
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
                    callable_syntax: false,
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
        let mut prefix = String::new();
        let mut sites = Vec::new();
        let segments = ty
            .path()
            .map(|path| path.segments().collect::<Vec<_>>())
            .unwrap_or_default()
            .into_iter()
            .chain(ty.name());
        for segment in segments {
            if let Some(name) = segment.text() {
                if !prefix.is_empty() {
                    prefix.push_str("::");
                }
                prefix.push_str(&name);
                sites.push((prefix.clone(), token_span(&segment)));
            }
        }
        self.source_map.insert_type_path(id, sites);
        if let Some(name) = ty
            .path()
            .map(|path| token_span(&path))
            .or_else(|| ty.name().map(|name| token_span(&name)))
            .or_else(|| ty.qualified_type()?.member().map(|name| token_span(&name)))
        {
            self.source_map.insert_type_name(id, name);
        }
        if let Some(name) = ty
            .path()
            .and_then(|path| path.segments().last())
            .or_else(|| ty.name())
            .or_else(|| ty.qualified_type()?.member())
        {
            self.source_map.insert_type_terminal(id, token_span(&name));
        }
        id
    }

    /// Collects associated-name/type pairs in source order, preserving missing values as type placeholders.
    pub(crate) fn lower_associated_bindings(
        &mut self,
        list: &GenericArgList,
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
