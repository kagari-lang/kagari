//! Exhaustive typed-syntax lowering and separate path/name/terminal navigation sites.

use kagari_common::span::Span;
use kagari_syntax::ast::{
    borrow::Ast,
    misc::{GenericArgList, Name, Path},
    ty::{QualifiedType, TypeSyntax},
    views::TypeView,
};
use smallvec::SmallVec;

use crate::{
    hir::{
        ids::TypeRefId,
        ty::{TypeData, TypeKind},
    },
    lower::context::{Lowerer, syntax_span, token_span},
};

impl Lowerer {
    pub(crate) fn lower_type(&mut self, ty: &Ast<'_, TypeSyntax>) -> TypeRefId {
        let (kind, path, terminal) = match ty.view() {
            TypeView::Grouped(grouped) => {
                return grouped
                    .ty()
                    .map(|inner| self.lower_type(&inner))
                    .unwrap_or_else(|| {
                        self.alloc_type(
                            syntax_span(ty),
                            TypeData {
                                kind: TypeKind::Named("<missing>".into()),
                            },
                        )
                    });
            }
            TypeView::Named(path) => (
                TypeKind::Named(path.text().unwrap_or_default()),
                Some(path),
                None,
            ),
            TypeView::Applied(applied) => {
                let arguments = applied.arguments();
                let path = applied.path();
                let kind = TypeKind::Generic {
                    name: path.and_then(|path| path.text()).unwrap_or_default(),
                    args: arguments.args().map(|ty| self.lower_type(&ty)).collect(),
                    bindings: self.lower_associated_bindings(&arguments),
                    callable_syntax: false,
                    positional_after_binding: arguments.positional_after_binding(),
                };
                (kind, path, None)
            }
            TypeView::Never(_) => (TypeKind::Named("!".into()), None, None),
            TypeView::Tuple(tuple) => (
                TypeKind::Tuple(
                    tuple
                        .element_types()
                        .map(|ty| self.lower_type(&ty))
                        .collect::<SmallVec<[_; 4]>>(),
                ),
                None,
                None,
            ),
            TypeView::Array(array) => (
                TypeKind::Array(
                    array
                        .element_type()
                        .map(|ty| self.lower_type(&ty))
                        .unwrap_or_else(|| self.synthetic_named_type("<missing>")),
                ),
                None,
                None,
            ),
            TypeView::Function(function) => (
                TypeKind::Function {
                    params: function.params().map(|ty| self.lower_type(&ty)).collect(),
                    result: function
                        .result()
                        .map(|ty| self.lower_type(&ty))
                        .unwrap_or_else(|| self.synthetic_named_type("<missing>")),
                },
                None,
                None,
            ),
            TypeView::Qualified(qualified) => {
                (self.projection_kind(&qualified), None, qualified.member())
            }
            TypeView::Error(_) => (TypeKind::Named("<missing>".into()), None, None),
        };
        self.written_type(kind, syntax_span(ty), path, terminal)
    }

    pub(crate) fn lower_qualified_type(&mut self, qualified: &Ast<'_, QualifiedType>) -> TypeRefId {
        let kind = self.projection_kind(qualified);
        self.written_type(kind, syntax_span(qualified), None, qualified.member())
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

    fn written_type(
        &mut self,
        kind: TypeKind,
        span: Span,
        path: Option<Ast<'_, Path>>,
        terminal: Option<Ast<'_, Name>>,
    ) -> TypeRefId {
        let id = self.alloc_type(span, TypeData { kind });
        let mut prefix = String::new();
        let mut sites = Vec::new();
        if let Some(path) = path {
            for segment in path.segments() {
                if let Some(name) = segment.text() {
                    if !prefix.is_empty() {
                        prefix.push_str("::");
                    }
                    prefix.push_str(&name);
                    sites.push((prefix.clone(), token_span(&segment)));
                }
            }
        }
        self.source_map.insert_type_path(id, sites);
        if let Some(span) = path
            .map(|path| token_span(&path))
            .or_else(|| terminal.map(|name| token_span(&name)))
        {
            self.source_map.insert_type_name(id, span);
        }
        if let Some(name) = path.and_then(|path| path.segments().last()).or(terminal) {
            self.source_map.insert_type_terminal(id, token_span(&name));
        }
        id
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
