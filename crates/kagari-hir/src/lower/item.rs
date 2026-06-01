use kagari_syntax::ast;

use crate::builtin::surface;
use crate::hir::{
    BlockData, ConstItem, Enum, Export, ExportItem, Field, Function, FunctionKind, GenericParam,
    Impl, ImplMethod, Item, ModuleDecl, Param, StandardImport, StandardImportTarget, Struct,
    TraitBound, TraitDef, TraitMethod, TraitRef, TypeRefId, Variant, Visibility, Writeability,
};
use crate::lower::context::{Lowerer, syntax_span};

impl Lowerer {
    pub(crate) fn lower_module(&mut self, module: &ast::SourceFile) {
        for item in module.items() {
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

        let top_level_statements = module
            .statements()
            .map(|stmt| self.lower_stmt(&stmt))
            .collect::<Vec<_>>();
        let tail_expr = module.tail_expr().map(|expr| self.lower_expr(&expr));
        if !top_level_statements.is_empty() || tail_expr.is_some() {
            let body = self.alloc_block(
                syntax_span(module),
                BlockData {
                    statements: top_level_statements.into(),
                    tail_expr,
                },
            );
            let id = self.source_map.push_function(syntax_span(module));
            self.module.module_init = Some(id);
            self.module.functions.push(Function {
                id,
                kind: FunctionKind::ModuleInit,
                visibility: Visibility::Private,
                name: "__module_init__".to_owned(),
                generic_params: Vec::new(),
                bounds: Vec::new(),
                params: Vec::new(),
                return_type: None,
                body,
            });
        }
    }

    fn lower_module_decl(&mut self, module_def: &ast::ModuleDef) -> ModuleDecl {
        ModuleDecl {
            id: self.source_map.push_module(syntax_span(module_def)),
            visibility: if module_def.is_pub() {
                Visibility::Public
            } else {
                Visibility::Private
            },
            name: module_def.name_text().unwrap_or_default(),
            inline: module_def.block().is_some(),
        }
    }

    fn lower_use_decl(&mut self, use_decl: &ast::UseDecl) {
        let Some(tree) = use_decl.tree() else {
            return;
        };
        let visibility = if use_decl.is_pub() {
            Visibility::Public
        } else {
            Visibility::Private
        };
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
            self.lower_standard_import(
                visibility,
                &path,
                tree.alias().and_then(|alias| alias.text()),
            );
            return;
        }

        for child in nested {
            self.lower_use_tree(visibility, Some(path.clone()), &child);
        }
    }

    fn lower_standard_import(&mut self, visibility: Visibility, path: &str, alias: Option<String>) {
        let Some(target) = standard_import_target(path) else {
            return;
        };
        let Some(default_alias) = path.rsplit("::").next() else {
            return;
        };
        let alias = alias.unwrap_or_else(|| default_alias.to_owned());
        if alias.is_empty() {
            return;
        }
        if visibility == Visibility::Public {
            self.module.exports.push(Export {
                name: alias.clone(),
                item: match target {
                    StandardImportTarget::Module(module) => ExportItem::StandardModule(module),
                    StandardImportTarget::Function(intrinsic) => {
                        ExportItem::StandardFunction(intrinsic)
                    }
                },
            });
        }
        self.module.standard_imports.push(StandardImport {
            visibility,
            alias,
            target,
        });
    }

    fn lower_trait(&mut self, trait_def: &ast::TraitDef) -> TraitDef {
        let id = self.source_map.push_trait(syntax_span(trait_def));
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
            visibility: if trait_def.is_pub() {
                Visibility::Public
            } else {
                Visibility::Private
            },
            name: trait_def.name_text().unwrap_or_default(),
            generic_params,
            methods,
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
                .and_then(|trait_ref| trait_ref.path_text()),
            for_type,
            bounds: impl_block
                .where_clause()
                .map(|where_clause| self.lower_where_clause(&where_clause))
                .unwrap_or_default(),
            methods,
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
        Function {
            id,
            kind,
            visibility: if method.is_pub() {
                Visibility::Public
            } else {
                Visibility::Private
            },
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
        }
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
                            .map(|name| syntax_span(&name))
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
        let params = function
            .param_list()
            .map(|param_list| {
                param_list
                    .params()
                    .map(|param| Param {
                        id: self.source_map.push_param(
                            param
                                .name()
                                .map(|name| syntax_span(&name))
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

        Function {
            id,
            kind: FunctionKind::User,
            visibility: if function.is_pub() {
                Visibility::Public
            } else {
                Visibility::Private
            },
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
        }
    }

    fn lower_generic_params(&mut self, params: &ast::GenericParamList) -> Vec<GenericParam> {
        params
            .params()
            .map(|param| GenericParam {
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
            .map(|predicate| TraitBound {
                target: predicate.name_text().unwrap_or_default(),
                traits: predicate
                    .bounds()
                    .map(|bounds| self.lower_trait_refs(bounds.bounds()))
                    .unwrap_or_default(),
            })
            .collect::<Vec<_>>()
    }

    fn lower_trait_refs(&mut self, refs: impl Iterator<Item = ast::TraitRef>) -> Vec<TraitRef> {
        refs.map(|trait_ref| TraitRef {
            name: trait_ref.path_text().unwrap_or_default(),
        })
        .collect::<Vec<_>>()
    }

    fn lower_const(&mut self, const_def: &ast::ConstDef) -> ConstItem {
        ConstItem {
            id: self.source_map.push_const(syntax_span(const_def)),
            visibility: if const_def.is_pub() {
                Visibility::Public
            } else {
                Visibility::Private
            },
            name: const_def.name_text().unwrap_or_default(),
            ty: const_def.ty().map(|ty| self.lower_type(&ty)),
            initializer: const_def
                .initializer()
                .map(|expr| self.lower_expr(&expr))
                .unwrap_or_else(|| self.synthetic_name_expr("<missing>")),
        }
    }

    fn lower_struct(&mut self, struct_def: &ast::StructDef) -> Struct {
        let id = self.source_map.push_struct(syntax_span(struct_def));
        let fields = struct_def
            .field_list()
            .map(|field_list| {
                field_list
                    .fields()
                    .map(|field| Field {
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
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Struct {
            id,
            visibility: if struct_def.is_pub() {
                Visibility::Public
            } else {
                Visibility::Private
            },
            name: struct_def.name_text().unwrap_or_default(),
            fields,
            methods: Vec::new(),
            impls: Vec::new(),
        }
    }

    fn lower_enum(&mut self, enum_def: &ast::EnumDef) -> Enum {
        let id = self.source_map.push_enum(syntax_span(enum_def));
        let variants = enum_def
            .variant_list()
            .map(|variant_list| {
                variant_list
                    .variants()
                    .map(|variant| Variant {
                        name: variant.name_text().unwrap_or_default(),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Enum {
            id,
            visibility: if enum_def.is_pub() {
                Visibility::Public
            } else {
                Visibility::Private
            },
            name: enum_def.name_text().unwrap_or_default(),
            variants,
            methods: Vec::new(),
            impls: Vec::new(),
        }
    }
}

fn standard_import_target(path: &str) -> Option<StandardImportTarget> {
    if let Some(module) = surface::standard_module(path).map(|spec| spec.kind) {
        return Some(StandardImportTarget::Module(module));
    }

    let (module_path, function_name) = path.rsplit_once("::")?;
    let module = surface::standard_module(module_path)?.kind;
    surface::standard_function(module, function_name)
        .map(|function| StandardImportTarget::Function(function.intrinsic))
}
