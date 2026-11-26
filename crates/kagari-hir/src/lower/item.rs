//! Declaration lowering, owner switching and use-tree flattening.
//!
//! Top-level headers enter `Module` collections; methods/defaults can also allocate
//! function or constant slots there. Inline module declarations record a header only:
//! analysis prepares their separate source units. `pub use ...::*` retains visibility
//! on the import leaf and is expanded by import analysis, not into a separate export list.

use crate::{
    hir::{
        attribute::{AssociatedOwner, AttributeTarget},
        ids::{
            BodyOwner, ConstId, EnumId, FieldId, FieldOwner, FunctionId, GenericParamId, HirOwner,
            ImplId, ModuleId, NativeTypeId, ParamId, StaticId, StructId, TraitId, TraitMethodId,
            TypeAliasId, TypeRefId, UseId, VariantId,
        },
        item::{
            Item,
            adt::{Enum, Field, NativeTypeDeclaration, Struct, Variant},
            alias::TypeAlias,
            behavior::{
                AssociatedConst, AssociatedType, GenericParam, Impl, ImplMethod, TraitBound,
                TraitDef, TraitMethod, TraitRef,
            },
            function::{Function, FunctionImplementation, FunctionKind, NativeContract, Param},
            module::{Import, ImportLeaf, ModuleDecl, UseDecl as HirUseDecl},
            storage::{ConstItem, ConstOwner, StaticItem},
        },
        ty::TypeKind,
        writeability::Writeability,
    },
    lower::context::{self, Lowerer, syntax_span, token_span},
};
use kagari_syntax::ast::{
    borrow::Ast,
    item::{
        AssociatedType as AstAssociatedType, ConstDef, EnumDef, FnDef, ImplBlock, MethodDef,
        ModuleDef, NativeTypeDef, StaticDef, StructDef, TraitDef as AstTraitDef,
        TypeAlias as AstTypeAlias, UseDecl, UseTree, VisibilityKind as AstVisibility,
    },
    misc::{FieldList, GenericParamList, ParamList, TraitRef as AstTraitRef, WhereClause},
    module::ModuleSyntax,
    views::ItemView,
};
use kagari_types::{declaration::VariantShape, visibility::Visibility};

use kagari_common::span::Span;

use smallvec::SmallVec;

fn lower_visibility(visibility: AstVisibility) -> Visibility {
    match visibility {
        AstVisibility::Private => Visibility::Private,
        AstVisibility::PublicSuper => Visibility::PublicSuper,
        AstVisibility::PublicCrate => Visibility::PublicCrate,
        AstVisibility::Public => Visibility::Public,
    }
}

impl Lowerer {
    /// Collects top-level declarations; cancellation leaves partial work for the caller to reject.
    pub(crate) fn lower_module(&mut self, module: &ModuleSyntax) {
        for item in module.items() {
            if self.cancel.check().is_err() {
                break;
            }
            match item {
                ItemView::Error(_) => {}
                ItemView::TypeAlias(declaration) => {
                    let item = self.lower_type_alias(&declaration);
                    self.module.items.push(Item::TypeAlias(item.id));
                    self.module.type_aliases.push(item);
                }
                ItemView::NativeTypeDef(declaration) => {
                    let item = self.lower_native_type(&declaration);
                    self.module.items.push(Item::NativeType(item.id));
                    self.module.native_types.push(item);
                }
                ItemView::ModuleDef(module_def) => {
                    let hir_module = self.lower_module_decl(&module_def);
                    self.module.items.push(Item::Module(hir_module.id));
                    self.module.modules.push(hir_module);
                }
                ItemView::UseDecl(use_decl) => self.lower_use_decl(&use_decl),
                ItemView::TraitDef(trait_def) => {
                    let hir_trait = self.lower_trait(&trait_def);
                    self.module.items.push(Item::Trait(hir_trait.id));
                    self.module.traits.push(hir_trait);
                }
                ItemView::ImplBlock(impl_block) => {
                    let hir_impl = self.lower_impl(&impl_block);
                    self.module.items.push(Item::Impl(hir_impl.id));
                    self.module.impls.push(hir_impl);
                }
                ItemView::FnDef(function) => {
                    let id = self.lower_function(&function, FunctionKind::User);
                    self.module.items.push(Item::Function(id));
                }
                ItemView::ConstDef(const_def) => {
                    let hir_const = self.lower_const(&const_def, None);
                    self.module.items.push(Item::Const(hir_const.id));
                    self.module.consts.push(hir_const);
                }
                ItemView::StaticDef(definition) => {
                    let item = self.lower_static(&definition);
                    self.module.items.push(Item::Static(item.id));
                    self.module.statics.push(item);
                }
                ItemView::StructDef(struct_def) => {
                    let hir_struct = self.lower_struct(&struct_def);
                    self.module.items.push(Item::Struct(hir_struct.id));
                    self.module.structs.push(hir_struct);
                }
                ItemView::EnumDef(enum_def) => {
                    let hir_enum = self.lower_enum(&enum_def);
                    self.module.items.push(Item::Enum(hir_enum.id));
                    self.module.enums.push(hir_enum);
                }
            }
        }
    }

    fn lower_type_alias(&mut self, declaration: &Ast<'_, AstTypeAlias>) -> TypeAlias {
        let id = TypeAliasId::new(self.module.type_aliases.len());
        self.attach_attributes(*declaration, AttributeTarget::Item(Item::TypeAlias(id)));
        TypeAlias {
            origin: self.origin(syntax_span(declaration)),
            name_span: declaration.name().as_ref().map(token_span),
            id,
            visibility: lower_visibility(declaration.visibility()),
            name: declaration.name_text().unwrap_or_default(),
            generic_params: declaration
                .generic_params()
                .map(|params| self.lower_generic_params(&params))
                .unwrap_or_default(),
            bounds: declaration
                .where_clause()
                .map(|clause| self.lower_where_clause(&clause))
                .unwrap_or_default(),
            target: declaration.ty().map(|ty| self.lower_type(&ty)),
        }
    }

    fn lower_native_type(&mut self, declaration: &Ast<'_, NativeTypeDef>) -> NativeTypeDeclaration {
        let id = NativeTypeId::new(self.module.native_types.len());
        self.attach_attributes(*declaration, AttributeTarget::Item(Item::NativeType(id)));
        NativeTypeDeclaration {
            origin: self.origin(syntax_span(declaration)),
            name_span: declaration.name().as_ref().map(token_span),
            is_host: declaration.is_host(),
            id,
            visibility: lower_visibility(declaration.visibility()),
            name: declaration.name_text().unwrap_or_default(),
            generic_params: declaration
                .generic_params()
                .map(|params| self.lower_generic_params(&params))
                .unwrap_or_default(),
            bounds: declaration
                .where_clause()
                .map(|clause| self.lower_where_clause(&clause))
                .unwrap_or_default(),
            trait_bounds: declaration
                .bounds()
                .map(|bounds| self.lower_trait_refs(bounds.bounds()))
                .unwrap_or_default(),
            fields: self.lower_fields(declaration.field_list(), FieldOwner::NativeType(id)),
        }
    }

    fn lower_module_decl(&mut self, module_def: &Ast<'_, ModuleDef>) -> ModuleDecl {
        let id = ModuleId::new(self.module.modules.len());
        self.attach_attributes(*module_def, AttributeTarget::Item(Item::Module(id)));
        ModuleDecl {
            origin: self.origin(syntax_span(module_def)),
            name_span: module_def.name().as_ref().map(token_span),
            id,
            visibility: lower_visibility(module_def.visibility()),
            name: module_def.name_text().unwrap_or_default(),
            inline: module_def.body().is_some(),
        }
    }

    fn lower_use_decl(&mut self, use_decl: &Ast<'_, UseDecl>) {
        let id = UseId::new(self.module.uses.len());
        self.attach_attributes(*use_decl, AttributeTarget::Use(id));
        let start = self.module.imports.len();
        if let Some(tree) = use_decl.tree() {
            self.lower_use_tree(
                lower_visibility(use_decl.visibility()),
                None,
                &tree,
                token_span(&tree),
            );
        }
        self.module.uses.push(HirUseDecl {
            origin: self.origin(syntax_span(use_decl)),
            id,
            imports: start..self.module.imports.len(),
        });
    }

    /// Flattens a recursive use tree by extending a copied prefix for each child.
    ///
    /// For `use a::{b, c::d as e}`, leaves become `a::b`/`b` and `a::c::d`/`e`.
    /// `base` contains written segments. Only leaves
    /// allocate imports; this traversal does not resolve names or generate namespace
    /// member imports. Recovery without a root tree yields no leaf.
    fn lower_use_tree(
        &mut self,
        visibility: Visibility,
        base: Option<Vec<String>>,
        tree: &Ast<'_, UseTree>,
        root_span: Span,
    ) {
        let mut path = base.unwrap_or_default();
        if let Some(local) = tree.path() {
            for segment in local.segments() {
                if let Some(name) = segment.text() {
                    path.push(name);
                }
            }
        }
        let nested = tree.nested_trees().collect::<Vec<_>>();
        if nested.is_empty() {
            self.lower_import(visibility, path, tree, root_span);
        } else {
            for child in nested {
                self.lower_use_tree(visibility, Some(path.clone()), &child, root_span);
            }
        }
    }

    /// Stores one leaf; public members derive from resolved bindings.
    fn lower_import(
        &mut self,
        visibility: Visibility,
        path: Vec<String>,
        tree: &Ast<'_, UseTree>,
        root_span: Span,
    ) {
        let explicit = tree.alias().and_then(|alias| alias.text());
        let glob = tree.is_glob();

        self.module.imports.push(Import {
            namespace: None,
            visibility,
            kind: if glob {
                ImportLeaf::Glob
            } else {
                ImportLeaf::Named { alias: explicit }
            },
            segments: path,
            span: token_span(tree),
            root_span,
        });
    }

    pub(crate) fn lower_trait(&mut self, trait_def: &Ast<'_, AstTraitDef>) -> TraitDef {
        let id = TraitId::new(self.module.traits.len());
        self.attach_attributes(*trait_def, AttributeTarget::Item(Item::Trait(id)));
        let generic_params = trait_def
            .generic_params()
            .map(|params| self.lower_generic_params(&params))
            .unwrap_or_default();
        let methods = trait_def
            .methods()
            .enumerate()
            .map(|(slot, method)| self.lower_trait_method(&method, &generic_params, id, slot))
            .collect::<Vec<_>>();
        TraitDef {
            origin: self.origin(syntax_span(trait_def)),
            name_span: trait_def.name().as_ref().map(token_span),
            conversion_adapter: None,
            associated_consts: trait_def
                .associated_consts()
                .enumerate()
                .map(|(index, item)| {
                    self.lower_associated_const(&item, AssociatedOwner::Trait(id), index)
                })
                .collect(),
            id,
            supertraits: trait_def
                .supertraits()
                .map(|bounds| self.lower_trait_refs(bounds.bounds()))
                .unwrap_or_default(),
            visibility: lower_visibility(trait_def.visibility()),
            name: trait_def.name_text().unwrap_or_default(),
            generic_params,
            bounds: trait_def
                .where_clause()
                .map(|clause| self.lower_where_clause(&clause))
                .unwrap_or_default(),
            methods,
            associated_types: trait_def
                .associated_types()
                .enumerate()
                .map(|(index, item)| {
                    self.lower_associated_type(&item, AssociatedOwner::Trait(id), index)
                })
                .collect(),
        }
    }

    fn lower_impl(&mut self, impl_block: &Ast<'_, ImplBlock>) -> Impl {
        let id = ImplId::new(self.module.impls.len());
        self.attach_attributes(*impl_block, AttributeTarget::Item(Item::Impl(id)));
        let generic_params = impl_block
            .generic_params()
            .map(|params| self.lower_generic_params(&params))
            .unwrap_or_default();
        let for_type = impl_block
            .target_type()
            .map(|target_type| self.lower_type(&target_type));
        let methods = impl_block
            .methods()
            .map(|method| self.lower_impl_method(&method, for_type, &generic_params))
            .collect::<Vec<_>>();
        Impl {
            origin: self.origin(syntax_span(impl_block)),
            id,
            associated_consts: impl_block
                .associated_consts()
                .enumerate()
                .map(|(index, item)| {
                    self.lower_associated_const(&item, AssociatedOwner::Impl(id), index)
                })
                .collect(),
            generic_params,
            trait_ref: impl_block
                .trait_ref()
                .map(|trait_ref| self.lower_trait_ref(&trait_ref)),
            for_type,
            bounds: impl_block
                .where_clause()
                .map(|where_clause| self.lower_where_clause(&where_clause))
                .unwrap_or_default(),
            methods,
            associated_types: impl_block
                .associated_types()
                .enumerate()
                .map(|(index, item)| {
                    self.lower_associated_type(&item, AssociatedOwner::Impl(id), index)
                })
                .collect(),
        }
    }

    fn lower_associated_type(
        &mut self,
        item: &Ast<'_, AstAssociatedType>,
        owner: AssociatedOwner,
        index: usize,
    ) -> AssociatedType {
        self.attach_attributes(*item, AttributeTarget::AssociatedType { owner, index });
        let name = item.name_text().unwrap_or_default();
        let name_ref = self.alloc_type(
            item.name()
                .as_ref()
                .map(token_span)
                .unwrap_or_else(|| syntax_span(item)),
            TypeKind::Named(name.clone()),
        );
        AssociatedType {
            origin: self.origin(syntax_span(item)),
            name_span: item.name().as_ref().map(token_span),
            generic_params: item
                .generic_params()
                .map(|params| self.lower_generic_params(&params))
                .unwrap_or_default(),
            parameter_bounds: item
                .where_clause()
                .map(|clause| self.lower_where_clause(&clause))
                .unwrap_or_default(),
            name,
            name_ref,
            ty: item.ty().map(|ty| self.lower_type(&ty)),
            bounds: item
                .bounds()
                .map(|bounds| self.lower_trait_refs(bounds.bounds()))
                .unwrap_or_default(),
        }
    }

    fn lower_trait_method(
        &mut self,
        method: &Ast<'_, MethodDef>,
        inherited_generics: &[GenericParamId],
        owner: TraitId,
        slot: usize,
    ) -> TraitMethod {
        let function =
            self.lower_method_function(method, FunctionKind::TraitMethod, None, inherited_generics);
        let id = TraitMethodId::new(self.module.nodes.arena(), owner, slot);
        let function_id = function.id;
        self.finish_function(function);
        TraitMethod {
            origin: self.origin(syntax_span(method)),
            name_span: method.name().as_ref().map(token_span),
            has_default: method.is_native() || method.body().is_some(),
            id,
            name: method.name_text().unwrap_or_default(),
            function: function_id,
        }
    }

    fn lower_impl_method(
        &mut self,
        method: &Ast<'_, MethodDef>,
        receiver_ty: Option<TypeRefId>,
        inherited_generics: &[GenericParamId],
    ) -> ImplMethod {
        let function = self.lower_method_function(
            method,
            FunctionKind::ImplMethod,
            receiver_ty,
            inherited_generics,
        );
        let function_id = function.id;
        self.finish_function(function);
        ImplMethod {
            name: method.name_text().unwrap_or_default(),
            function: function_id,
        }
    }

    /// Builds a method's function slot under its owner, combining inherited and method generic binders.
    fn lower_method_function(
        &mut self,
        method: &Ast<'_, MethodDef>,
        kind: FunctionKind,
        receiver_ty: Option<TypeRefId>,
        inherited_generics: &[GenericParamId],
    ) -> Function {
        let id = self.reserve_function();
        self.attach_attributes(*method, AttributeTarget::Item(Item::Function(id)));
        let previous_owner = self.set_owner(HirOwner::Body(BodyOwner::Function(id)));
        let params = self.lower_method_params(id, &method.param_list(), receiver_ty);
        let mut generic_params = inherited_generics.to_vec();
        generic_params.extend(
            method
                .generic_params()
                .map(|params| self.lower_generic_params(&params))
                .unwrap_or_default(),
        );
        let result = Function {
            origin: self.origin(syntax_span(method)),
            name_span: method.name().as_ref().map(token_span),
            id,
            kind,
            is_async: method.is_async(),
            visibility: lower_visibility(method.visibility()),
            name: method.name_text().unwrap_or_default(),
            generic_params,
            bounds: method
                .where_clause()
                .map(|where_clause| self.lower_where_clause(&where_clause))
                .unwrap_or_default(),
            params,
            return_type: method.return_type().map(|ty| self.lower_type(&ty)),
            implementation: if method.is_native() {
                FunctionImplementation::Native(NativeContract::default())
            } else {
                method
                    .body()
                    .map_or(FunctionImplementation::Required, |body| {
                        FunctionImplementation::Script(self.lower_block(&body))
                    })
            },
        };
        self.set_owner(previous_owner);
        result
    }

    fn lower_method_params(
        &mut self,
        function: FunctionId,
        param_list: &Ast<'_, ParamList>,
        receiver_ty: Option<TypeRefId>,
    ) -> Vec<Param> {
        param_list
            .params()
            .enumerate()
            .map(|(index, param)| {
                let name = param.name_text().unwrap_or_default();
                let ty = if name == "self" {
                    receiver_ty.unwrap_or_else(|| self.synthetic_named_type("Self"))
                } else {
                    param
                        .ty()
                        .map(|ty| self.lower_type(&ty))
                        .unwrap_or_else(|| self.synthetic_named_type("<missing>"))
                };
                let id = ParamId::new(
                    self.module.nodes.arena(),
                    HirOwner::Body(BodyOwner::Function(function)),
                    index,
                );
                Param {
                    origin: self.origin(syntax_span(&param)),
                    name_span: param.name().as_ref().map(token_span),
                    id,
                    name,
                    ty,
                }
            })
            .collect::<Vec<_>>()
    }

    /// Allocates a function ID, lowers signature/body under its owner, then restores the prior owner.
    pub(crate) fn lower_function(
        &mut self,
        function: &Ast<'_, FnDef>,
        kind: FunctionKind,
    ) -> FunctionId {
        let id = self.reserve_function();
        self.attach_attributes(*function, AttributeTarget::Item(Item::Function(id)));
        let previous_owner = self.set_owner(HirOwner::Body(BodyOwner::Function(id)));
        let params = function
            .param_list()
            .params()
            .enumerate()
            .map(|(index, param)| {
                let parameter = ParamId::new(
                    self.module.nodes.arena(),
                    HirOwner::Body(BodyOwner::Function(id)),
                    index,
                );
                Param {
                    origin: self.origin(syntax_span(&param)),
                    name_span: param.name().as_ref().map(token_span),
                    id: parameter,
                    name: param.name_text().unwrap_or_default(),
                    ty: param
                        .ty()
                        .map(|ty| self.lower_type(&ty))
                        .unwrap_or_else(|| self.synthetic_named_type("<missing>")),
                }
            })
            .collect();

        let result = Function {
            origin: self.origin(syntax_span(function)),
            name_span: function.name().as_ref().map(token_span),
            id,
            kind,
            is_async: function.is_async(),
            visibility: lower_visibility(function.visibility()),
            name: function.name_text().unwrap_or_default(),
            generic_params: function
                .generic_params()
                .map(|params| self.lower_generic_params(&params))
                .unwrap_or_default(),
            bounds: function
                .where_clause()
                .map(|where_clause| self.lower_where_clause(&where_clause))
                .unwrap_or_default(),
            params,
            return_type: function.return_type().map(|ty| self.lower_type(&ty)),
            implementation: if function.is_native() {
                FunctionImplementation::Native(NativeContract::default())
            } else {
                function
                    .body()
                    .map_or(FunctionImplementation::Required, |body| {
                        FunctionImplementation::Script(self.lower_block(&body))
                    })
            },
        };
        self.set_owner(previous_owner);
        self.finish_function(result);
        id
    }

    fn lower_generic_params(&mut self, params: &Ast<'_, GenericParamList>) -> Vec<GenericParamId> {
        params
            .params()
            .map(|param| {
                let origin = self.origin(syntax_span(&param));
                let name_span = param.name().as_ref().map(token_span);
                let name = param.name_text().unwrap_or_default();
                let bounds = param
                    .bounds()
                    .map(|bounds| self.lower_trait_refs(bounds.bounds()))
                    .unwrap_or_default();
                self.alloc_generic_param(GenericParam {
                    origin,
                    name_span,
                    name,
                    bounds,
                })
            })
            .collect()
    }

    fn lower_where_clause(&mut self, where_clause: &Ast<'_, WhereClause>) -> Vec<TraitBound> {
        where_clause
            .predicates()
            .map(|predicate| {
                let target_ref = predicate
                    .target_type()
                    .map(|ty| self.lower_type(&ty))
                    .unwrap_or_else(|| self.synthetic_named_type("<missing>"));
                TraitBound {
                    target: predicate.name_text().unwrap_or_default(),
                    target_ref,
                    traits: predicate
                        .bounds()
                        .map(|bounds| self.lower_trait_refs(bounds.bounds()))
                        .unwrap_or_default(),
                }
            })
            .collect::<Vec<_>>()
    }

    fn lower_trait_refs<'a>(
        &mut self,
        refs: impl Iterator<Item = Ast<'a, AstTraitRef>>,
    ) -> Vec<TraitRef> {
        refs.map(|reference| self.lower_trait_ref(&reference))
            .collect()
    }

    /// Encodes trait syntax as type syntax; callable inputs become a tuple argument and an `Output` binding.
    pub(crate) fn lower_trait_ref(&mut self, trait_ref: &Ast<'_, AstTraitRef>) -> TraitRef {
        let name = trait_ref.path_text().unwrap_or_default();
        let args: SmallVec<[TypeRefId; 4]> = trait_ref
            .generic_args()
            .map(|args| args.args().map(|arg| self.lower_type(&arg)).collect())
            .unwrap_or_default();
        let kind = if let Some(callable) = trait_ref.callable() {
            let input_span = callable.input_span();
            let inputs = callable.inputs().map(|ty| self.lower_type(&ty)).collect();
            let tuple = self.alloc_type(input_span, TypeKind::Tuple(inputs));
            let output = if let Some(output) = callable.output() {
                self.lower_type(&output)
            } else {
                let end = token_span(trait_ref).end;
                self.alloc_type(
                    Span { start: end, end },
                    TypeKind::Tuple(Default::default()),
                )
            };
            TypeKind::Generic {
                name,
                args: smallvec::smallvec![tuple],
                bindings: vec![("Output".into(), output)],
                positional_after_binding: false,
                callable_syntax: true,
            }
        } else if trait_ref.generic_args().is_none() {
            TypeKind::Named(name)
        } else {
            let list = trait_ref.generic_args().expect("generic arguments");
            let bindings = self.lower_associated_bindings(&list);
            TypeKind::Generic {
                name,
                args,
                bindings,
                callable_syntax: false,
                positional_after_binding: list.positional_after_binding(),
            }
        };
        let ty = self.alloc_type(context::token_span(trait_ref), kind);

        TraitRef { ty }
    }

    /// Lowers annotation and initializer under a fresh constant body owner, restoring the caller's owner.
    fn lower_const(
        &mut self,
        const_def: &Ast<'_, ConstDef>,
        owner: Option<ConstOwner>,
    ) -> ConstItem {
        let id = ConstId::new(self.module.consts.len());
        // Associated members attach once to their inline slot, not the initializer row.
        if owner.is_none() {
            self.attach_attributes(*const_def, AttributeTarget::Item(Item::Const(id)));
        }
        let previous_owner = self.set_owner(HirOwner::Body(BodyOwner::Const(id)));
        let result = ConstItem {
            origin: self.origin(syntax_span(const_def)),
            name_span: const_def.name().as_ref().map(token_span),
            owner,
            id,
            visibility: lower_visibility(const_def.visibility()),
            name: const_def.name_text().unwrap_or_default(),
            ty: const_def.ty().map(|ty| self.lower_type(&ty)),
            initializer: const_def
                .initializer()
                .map(|expr| self.lower_expr(&expr))
                .unwrap_or_else(|| self.missing_expr()),
        };
        self.set_owner(previous_owner);
        result
    }

    fn lower_static(&mut self, definition: &Ast<'_, StaticDef>) -> StaticItem {
        let id = StaticId::new(self.module.statics.len());
        self.attach_attributes(*definition, AttributeTarget::Item(Item::Static(id)));
        let previous_owner = self.set_owner(HirOwner::Body(BodyOwner::Static(id)));
        let item = StaticItem {
            origin: self.origin(syntax_span(definition)),
            name_span: definition.name().as_ref().map(token_span),
            initializer_span: definition.initializer().as_ref().map(syntax_span),
            id,
            visibility: lower_visibility(definition.visibility()),
            name: definition
                .name()
                .and_then(|name| name.text())
                .unwrap_or_default(),
            writeability: if definition.is_var() {
                Writeability::Var
            } else {
                Writeability::Val
            },
            ty: definition.ty().map(|ty| self.lower_type(&ty)),
            initializer: definition
                .initializer()
                .map(|expr| self.lower_expr(&expr))
                .unwrap_or_else(|| self.missing_expr()),
        };
        self.set_owner(previous_owner);
        item
    }

    fn lower_associated_const(
        &mut self,
        item: &Ast<'_, ConstDef>,
        owner: AssociatedOwner,
        index: usize,
    ) -> AssociatedConst {
        self.attach_attributes(*item, AttributeTarget::AssociatedConst { owner, index });
        let name = item.name_text().unwrap_or_default();
        let name_ref = self.alloc_type(
            item.name()
                .map(|name| token_span(&name))
                .unwrap_or_else(|| syntax_span(item)),
            TypeKind::Named(name.clone()),
        );
        let (ty, initializer) = if item.initializer().is_some() {
            let initializer_owner = match owner {
                AssociatedOwner::Trait(id) => ConstOwner::Trait(id),
                AssociatedOwner::Impl(id) => ConstOwner::Impl(id),
            };
            let constant = self.lower_const(item, Some(initializer_owner));
            let ty = constant
                .ty
                .unwrap_or_else(|| self.synthetic_named_type("<missing>"));
            let id = constant.id;
            self.module.consts.push(constant);
            (ty, Some(id))
        } else {
            (
                item.ty()
                    .map(|ty| self.lower_type(&ty))
                    .unwrap_or_else(|| self.synthetic_named_type("<missing>")),
                None,
            )
        };
        AssociatedConst {
            origin: self.origin(syntax_span(item)),
            name_span: item.name().as_ref().map(token_span),
            name,
            name_ref,
            ty,
            initializer,
        }
    }

    fn lower_struct(&mut self, struct_def: &Ast<'_, StructDef>) -> Struct {
        let id = StructId::new(self.module.structs.len());
        self.attach_attributes(*struct_def, AttributeTarget::Item(Item::Struct(id)));
        let generic_params = struct_def
            .generic_params()
            .map(|params| self.lower_generic_params(&params))
            .unwrap_or_default();
        let fields = self.lower_fields(struct_def.field_list(), FieldOwner::Struct(id));

        Struct {
            origin: self.origin(syntax_span(struct_def)),
            name_span: struct_def.name().as_ref().map(token_span),
            id,
            visibility: lower_visibility(struct_def.visibility()),
            name: struct_def.name_text().unwrap_or_default(),
            generic_params,
            bounds: struct_def
                .where_clause()
                .map(|clause| self.lower_where_clause(&clause))
                .unwrap_or_default(),
            fields,
            impls: Vec::new(),
        }
    }

    fn lower_fields(&mut self, list: Option<Ast<'_, FieldList>>, owner: FieldOwner) -> Vec<Field> {
        list.map(|field_list| {
            field_list
                .fields()
                .enumerate()
                .map(|(slot, field)| {
                    let field_id = FieldId::new(self.module.nodes.arena(), owner, slot);
                    self.attach_attributes(field, AttributeTarget::Field(field_id));
                    Field {
                        origin: self.origin(syntax_span(&field)),
                        name_span: field.name().as_ref().map(token_span),
                        visibility: lower_visibility(field.visibility()),
                        id: field_id,
                        writeability: if field.is_var() {
                            Writeability::Var
                        } else {
                            Writeability::Val
                        },
                        name: field.name_text().unwrap_or_default(),
                        ty: field
                            .ty()
                            .map(|ty| self.lower_type(&ty))
                            .unwrap_or_else(|| self.synthetic_named_type("<missing>")),
                    }
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
    }

    fn lower_enum(&mut self, enum_def: &Ast<'_, EnumDef>) -> Enum {
        let id = EnumId::new(self.module.enums.len());
        self.attach_attributes(*enum_def, AttributeTarget::Item(Item::Enum(id)));
        let generic_params = enum_def
            .generic_params()
            .map(|params| self.lower_generic_params(&params))
            .unwrap_or_default();
        let variants = enum_def
            .variant_list()
            .map(|variant_list| {
                variant_list
                    .variants()
                    .enumerate()
                    .map(|(slot, variant)| {
                        let variant_id = VariantId::new(self.module.nodes.arena(), id, slot);
                        self.attach_attributes(variant, AttributeTarget::Variant(variant_id));
                        let (shape, payload) = if let Some(named) = variant.named_payload() {
                            let (labels, types) = named
                                .fields()
                                .map(|field| {
                                    (
                                        field.name().and_then(|n| n.text()).unwrap_or_default(),
                                        field.ty().map(|ty| self.lower_type(&ty)).unwrap_or_else(
                                            || self.synthetic_named_type("<missing>"),
                                        ),
                                    )
                                })
                                .unzip();
                            (VariantShape::Named(labels), types)
                        } else {
                            (
                                VariantShape::Positional,
                                variant
                                    .payload_types()
                                    .map(|types| self.lower_payload_types(&types))
                                    .unwrap_or_default(),
                            )
                        };
                        Variant {
                            shape,
                            origin: self.origin(syntax_span(&variant)),
                            name_span: variant.name().as_ref().map(token_span),
                            id: variant_id,
                            name: variant.name_text().unwrap_or_default(),
                            payload,
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Enum {
            origin: self.origin(syntax_span(enum_def)),
            name_span: enum_def.name().as_ref().map(token_span),
            id,
            visibility: lower_visibility(enum_def.visibility()),
            name: enum_def.name_text().unwrap_or_default(),
            generic_params,
            bounds: enum_def
                .where_clause()
                .map(|clause| self.lower_where_clause(&clause))
                .unwrap_or_default(),
            variants,
            impls: Vec::new(),
        }
    }
}
