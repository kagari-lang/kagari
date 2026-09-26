use kagari_syntax::ast;

use crate::hir::{
    BlockData, ConstItem, Enum, Export, ExportItem, Field, Function, FunctionKind, GenericParam,
    Impl, ImplMethod, Import, Item, ModuleDecl, Param, Struct, TraitBound, TraitDef, TraitMethod,
    TraitRef, TypeRefId, Variant, Visibility, Writeability,
};
use crate::hir::{BodyOwner, HirOwner};
use crate::lower::context::{Lowerer, syntax_span, token_span};

fn lower_visibility(visibility: ast::Visibility) -> Visibility {
    match visibility {
        ast::Visibility::Private => Visibility::Private,
        ast::Visibility::PublicSuper => Visibility::PublicSuper,
        ast::Visibility::Public => Visibility::Public,
    }
}

impl Lowerer {
    pub(crate) fn lower_module(&mut self, module: &ast::SourceFile) {
        for item in module.items() {
            if self.cancel.check().is_err() {
                break;
            }
            match item {
                ast::Item::ModuleDef(module_def) => {
                    let hir_module = self.lower_module_decl(&module_def);
                    if hir_module.visibility == Visibility::Public {
                        self.module.exports.push(Export {
                            name: hir_module.name.clone(),
                            item: ExportItem::Module(hir_module.id),
                        });
                    }
                    self.module.items.push(Item::Module(hir_module.id));
                    self.module.modules.push(hir_module);
                }
                ast::Item::UseDecl(use_decl) => self.lower_use_decl(&use_decl),
                ast::Item::TraitDef(trait_def) => {
                    let hir_trait = self.lower_trait(&trait_def);
                    if hir_trait.visibility == Visibility::Public {
                        self.module.exports.push(Export {
                            name: hir_trait.name.clone(),
                            item: ExportItem::Trait(hir_trait.id),
                        });
                    }
                    self.module.items.push(Item::Trait(hir_trait.id));
                    self.module.traits.push(hir_trait);
                }
                ast::Item::ImplBlock(impl_block) => {
                    let hir_impl = self.lower_impl(&impl_block);
                    self.module.items.push(Item::Impl(hir_impl.id));
                    self.module.impls.push(hir_impl);
                }
                ast::Item::FnDef(function) => {
                    let hir_function = self.lower_function(&function);
                    if hir_function.visibility == Visibility::Public {
                        self.module.exports.push(Export {
                            name: hir_function.name.clone(),
                            item: ExportItem::Function(hir_function.id),
                        });
                    }
                    self.module.items.push(Item::Function(hir_function.id));
                    self.module.functions.push(hir_function);
                }
                ast::Item::ConstDef(const_def) => {
                    let hir_const = self.lower_const(&const_def);
                    if hir_const.visibility == Visibility::Public {
                        self.module.exports.push(Export {
                            name: hir_const.name.clone(),
                            item: ExportItem::Const(hir_const.id),
                        });
                    }
                    self.module.items.push(Item::Const(hir_const.id));
                    self.module.consts.push(hir_const);
                }
                ast::Item::StructDef(struct_def) => {
                    let hir_struct = self.lower_struct(&struct_def);
                    if hir_struct.visibility == Visibility::Public {
                        self.module.exports.push(Export {
                            name: hir_struct.name.clone(),
                            item: ExportItem::Struct(hir_struct.id),
                        });
                    }
                    self.module.items.push(Item::Struct(hir_struct.id));
                    self.module.structs.push(hir_struct);
                }
                ast::Item::EnumDef(enum_def) => {
                    let hir_enum = self.lower_enum(&enum_def);
                    if hir_enum.visibility == Visibility::Public {
                        self.module.exports.push(Export {
                            name: hir_enum.name.clone(),
                            item: ExportItem::Enum(hir_enum.id),
                        });
                    }
                    self.module.items.push(Item::Enum(hir_enum.id));
                    self.module.enums.push(hir_enum);
                }
            }
        }
    }

    fn lower_module_decl(&mut self, module_def: &ast::ModuleDef) -> ModuleDecl {
        let id = self.source_map.push_module(syntax_span(module_def));
        if let Some(name) = module_def.name() {
            self.source_map
                .insert_item_name(crate::hir::Item::Module(id), token_span(&name));
        }
        ModuleDecl {
            id,
            visibility: lower_visibility(module_def.visibility()),
            name: module_def.name_text().unwrap_or_default(),
            inline: module_def.block().is_some(),
        }
    }

    fn lower_use_decl(&mut self, use_decl: &ast::UseDecl) {
        let Some(tree) = use_decl.tree() else {
            return;
        };
        let visibility = lower_visibility(use_decl.visibility());
        self.lower_use_tree(visibility, None, &tree);
    }

    fn lower_use_tree(
        &mut self,
        visibility: Visibility,
        base_path: Option<String>,
        tree: &ast::UseTree,
    ) {
        let path = match (base_path, tree.path().and_then(|path| path.text())) {
            (Some(base), Some(path)) => format!("{base}::{path}"),
            (Some(base), None) => base,
            (None, Some(path)) => path,
            (None, None) => String::new(),
        };

        let nested = tree.nested_trees().collect::<Vec<_>>();
        if nested.is_empty() {
            self.lower_import(
                visibility,
                &path,
                syntax_span(tree),
                tree.alias().and_then(|alias| alias.text()),
                tree.is_glob(),
            );
            return;
        }

        for child in nested {
            self.lower_use_tree(visibility, Some(path.clone()), &child);
        }
    }

    fn lower_import(
        &mut self,
        visibility: Visibility,
        path: &str,
        span: kagari_common::Span,
        alias: Option<String>,
        glob: bool,
    ) {
        let alias =
            alias.unwrap_or_else(|| path.rsplit("::").next().unwrap_or_default().to_owned());
        if visibility == Visibility::Public && !glob {
            self.module.exports.push(Export {
                name: alias.clone(),
                item: ExportItem::Import(self.module.imports.len()),
            });
        }
        self.module.imports.push(Import {
            visibility,
            alias,
            path: path.to_owned(),
            span,
            glob,
        });
    }
    fn lower_trait(&mut self, trait_def: &ast::TraitDef) -> TraitDef {
        let id = self.source_map.push_trait(syntax_span(trait_def));
        if let Some(name) = trait_def.name() {
            self.source_map
                .insert_item_name(crate::hir::Item::Trait(id), token_span(&name));
        }
        let generic_params = trait_def
            .generic_params()
            .map(|params| self.lower_generic_params(&params))
            .unwrap_or_default();
        let methods = trait_def
            .methods()
            .map(|method| self.lower_trait_method(&method, &generic_params))
            .collect::<Vec<_>>();
        TraitDef {
            id,
            supertraits: trait_def
                .supertraits()
                .map(|bounds| self.lower_trait_refs(bounds.bounds()))
                .unwrap_or_default(),
            visibility: lower_visibility(trait_def.visibility()),
            name: trait_def.name_text().unwrap_or_default(),
            generic_params,
            methods,
            associated_types: trait_def
                .associated_types()
                .map(|item| self.lower_associated_type(&item))
                .collect(),
        }
    }

    fn lower_impl(&mut self, impl_block: &ast::ImplBlock) -> Impl {
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
            id: self.source_map.push_impl(syntax_span(impl_block)),
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
                .map(|item| self.lower_associated_type(&item))
                .collect(),
        }
    }

    fn lower_associated_type(&mut self, item: &ast::AssociatedType) -> crate::hir::AssociatedType {
        let name = item.name_text().unwrap_or_default();
        let name_ref = self.alloc_type(
            item.name()
                .as_ref()
                .map(token_span)
                .unwrap_or_else(|| syntax_span(item)),
            crate::hir::TypeData {
                kind: crate::hir::TypeKind::Named(name.clone()),
            },
        );
        crate::hir::AssociatedType {
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
        method: &ast::MethodDef,
        inherited_generics: &[GenericParam],
    ) -> TraitMethod {
        let function =
            self.lower_method_function(method, FunctionKind::TraitMethod, None, inherited_generics);
        let id = self.source_map.push_trait_method(syntax_span(method));
        let function_id = function.id;
        self.module.functions.push(function);
        TraitMethod {
            id,
            name: method.name_text().unwrap_or_default(),
            receiver: crate::hir::ReceiverKind::Value,
            function: function_id,
        }
    }

    fn lower_impl_method(
        &mut self,
        method: &ast::MethodDef,
        receiver_ty: Option<TypeRefId>,
        inherited_generics: &[GenericParam],
    ) -> ImplMethod {
        let function = self.lower_method_function(
            method,
            FunctionKind::ImplMethod,
            receiver_ty,
            inherited_generics,
        );
        let function_id = function.id;
        self.module.functions.push(function);
        ImplMethod {
            name: method.name_text().unwrap_or_default(),
            function: function_id,
        }
    }

    fn lower_method_function(
        &mut self,
        method: &ast::MethodDef,
        kind: FunctionKind,
        receiver_ty: Option<TypeRefId>,
        inherited_generics: &[GenericParam],
    ) -> Function {
        let id = self.source_map.push_function(syntax_span(method));
        if let Some(name) = method.name() {
            self.source_map
                .insert_item_name(crate::hir::Item::Function(id), token_span(&name));
        }
        let previous_owner = self
            .source_map
            .set_owner(HirOwner::Body(BodyOwner::Function(id)));
        let params = method
            .param_list()
            .map(|param_list| self.lower_method_params(&param_list, receiver_ty))
            .unwrap_or_default();
        let mut generic_params = inherited_generics.to_vec();
        generic_params.extend(
            method
                .generic_params()
                .map(|params| self.lower_generic_params(&params))
                .unwrap_or_default(),
        );
        let result = Function {
            id,
            kind,
            visibility: lower_visibility(method.visibility()),
            name: method.name_text().unwrap_or_default(),
            generic_params,
            bounds: method
                .where_clause()
                .map(|where_clause| self.lower_where_clause(&where_clause))
                .unwrap_or_default(),
            params,
            return_type: method.return_type().map(|ty| self.lower_type(&ty)),
            body: method
                .body()
                .map(|body| self.lower_block(&body))
                .unwrap_or_else(|| {
                    self.alloc_block(
                        syntax_span(method),
                        BlockData {
                            statements: Default::default(),
                            tail_expr: None,
                        },
                    )
                }),
        };
        self.source_map.set_owner(previous_owner);
        result
    }

    fn lower_method_params(
        &mut self,
        param_list: &ast::ParamList,
        receiver_ty: Option<TypeRefId>,
    ) -> Vec<Param> {
        param_list
            .params()
            .map(|param| {
                let name = param.name_text().unwrap_or_default();
                let ty = if name == "self" {
                    receiver_ty.unwrap_or_else(|| self.synthetic_named_type("Self"))
                } else {
                    param
                        .ty()
                        .map(|ty| self.lower_type(&ty))
                        .unwrap_or_else(|| self.synthetic_named_type("<missing>"))
                };
                Param {
                    id: self.source_map.push_param(
                        param
                            .name()
                            .map(|name| token_span(&name))
                            .unwrap_or_else(|| syntax_span(&param)),
                    ),
                    writeability: Writeability::Val,
                    name,
                    ty,
                }
            })
            .collect::<Vec<_>>()
    }

    fn lower_function(&mut self, function: &ast::FnDef) -> Function {
        let id = self.source_map.push_function(syntax_span(function));
        if let Some(name) = function.name() {
            self.source_map
                .insert_item_name(crate::hir::Item::Function(id), token_span(&name));
        }
        let previous_owner = self
            .source_map
            .set_owner(HirOwner::Body(BodyOwner::Function(id)));
        let params = function
            .param_list()
            .map(|param_list| {
                param_list
                    .params()
                    .map(|param| Param {
                        id: self.source_map.push_param(
                            param
                                .name()
                                .map(|name| token_span(&name))
                                .unwrap_or_else(|| syntax_span(&param)),
                        ),
                        writeability: Writeability::Val,
                        name: param.name_text().unwrap_or_default(),
                        ty: param
                            .ty()
                            .map(|ty| self.lower_type(&ty))
                            .unwrap_or_else(|| self.synthetic_named_type("<missing>")),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let result = Function {
            id,
            kind: FunctionKind::User,
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
            body: function
                .body()
                .map(|body| self.lower_block(&body))
                .unwrap_or_else(|| {
                    self.alloc_block(
                        syntax_span(function),
                        BlockData {
                            statements: Default::default(),
                            tail_expr: None,
                        },
                    )
                }),
        };
        self.source_map.set_owner(previous_owner);
        result
    }

    fn lower_generic_params(&mut self, params: &ast::GenericParamList) -> Vec<GenericParam> {
        params
            .params()
            .map(|param| GenericParam {
                id: self.source_map.push_generic_param(
                    param
                        .name()
                        .map(|name| token_span(&name))
                        .unwrap_or_else(|| syntax_span(&param)),
                ),
                name: param.name_text().unwrap_or_default(),
                bounds: param
                    .bounds()
                    .map(|bounds| self.lower_trait_refs(bounds.bounds()))
                    .unwrap_or_default(),
            })
            .collect::<Vec<_>>()
    }

    fn lower_where_clause(&mut self, where_clause: &ast::WhereClause) -> Vec<TraitBound> {
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

    fn lower_trait_refs(&mut self, refs: impl Iterator<Item = ast::TraitRef>) -> Vec<TraitRef> {
        refs.map(|reference| self.lower_trait_ref(&reference))
            .collect()
    }

    pub(crate) fn lower_trait_ref(&mut self, trait_ref: &ast::TraitRef) -> TraitRef {
        let name = trait_ref.path_text().unwrap_or_default();
        let args: smallvec::SmallVec<[TypeRefId; 4]> = trait_ref
            .generic_args()
            .map(|args| args.args().map(|arg| self.lower_type(&arg)).collect())
            .unwrap_or_default();
        let kind = if trait_ref.generic_args().is_none() {
            crate::hir::TypeKind::Named(name)
        } else {
            let list = trait_ref.generic_args().expect("generic arguments");
            let bindings = self.lower_associated_bindings(&list);
            crate::hir::TypeKind::Generic {
                name,
                args,
                bindings,
                positional_after_binding: list.positional_after_binding(),
            }
        };
        let ty = self.alloc_type(
            crate::lower::context::token_span(trait_ref),
            crate::hir::TypeData { kind },
        );
        if let Some(name) = trait_ref.path().and_then(|path| path.segments().last()) {
            self.source_map.insert_type_name(ty, token_span(&name));
        }
        TraitRef { ty }
    }

    fn lower_const(&mut self, const_def: &ast::ConstDef) -> ConstItem {
        let id = self.source_map.push_const(syntax_span(const_def));
        if let Some(name) = const_def.name() {
            self.source_map
                .insert_item_name(crate::hir::Item::Const(id), token_span(&name));
        }
        let previous_owner = self
            .source_map
            .set_owner(HirOwner::Body(BodyOwner::Const(id)));
        let result = ConstItem {
            id,
            visibility: lower_visibility(const_def.visibility()),
            name: const_def.name_text().unwrap_or_default(),
            ty: const_def.ty().map(|ty| self.lower_type(&ty)),
            initializer: const_def
                .initializer()
                .map(|expr| self.lower_expr(&expr))
                .unwrap_or_else(|| self.missing_expr()),
        };
        self.source_map.set_owner(previous_owner);
        result
    }

    fn lower_struct(&mut self, struct_def: &ast::StructDef) -> Struct {
        let id = self.source_map.push_struct(syntax_span(struct_def));
        if let Some(name) = struct_def.name() {
            self.source_map
                .insert_item_name(crate::hir::Item::Struct(id), token_span(&name));
        }
        let generic_params = struct_def
            .generic_params()
            .map(|params| self.lower_generic_params(&params))
            .unwrap_or_default();
        let fields = struct_def
            .field_list()
            .map(|field_list| {
                field_list
                    .fields()
                    .enumerate()
                    .map(|(slot, field)| {
                        let field_id = crate::hir::FieldId::new(self.source_map.arena(), id, slot);
                        self.source_map.insert_field(
                            field_id,
                            field
                                .name()
                                .map(|name| token_span(&name))
                                .unwrap_or_else(|| syntax_span(&field)),
                        );
                        Field {
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
            .unwrap_or_default();

        Struct {
            id,
            visibility: lower_visibility(struct_def.visibility()),
            name: struct_def.name_text().unwrap_or_default(),
            generic_params,
            fields,
            methods: Vec::new(),
            impls: Vec::new(),
        }
    }

    fn lower_enum(&mut self, enum_def: &ast::EnumDef) -> Enum {
        let id = self.source_map.push_enum(syntax_span(enum_def));
        if let Some(name) = enum_def.name() {
            self.source_map
                .insert_item_name(crate::hir::Item::Enum(id), token_span(&name));
        }
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
                        let variant_id =
                            crate::hir::VariantId::new(self.source_map.arena(), id, slot);
                        self.source_map.insert_variant(
                            variant_id,
                            variant
                                .name()
                                .map(|name| token_span(&name))
                                .unwrap_or_else(|| syntax_span(&variant)),
                        );
                        Variant {
                            id: variant_id,
                            name: variant.name_text().unwrap_or_default(),
                            payload: variant
                                .payload_types()
                                .map(|types| types.types().map(|ty| self.lower_type(&ty)).collect())
                                .unwrap_or_default(),
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Enum {
            id,
            visibility: lower_visibility(enum_def.visibility()),
            name: enum_def.name_text().unwrap_or_default(),
            generic_params,
            variants,
            methods: Vec::new(),
            impls: Vec::new(),
        }
    }
}
