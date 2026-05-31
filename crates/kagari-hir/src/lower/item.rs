use kagari_syntax::ast;

use crate::hir::{
    BlockData, ConstItem, Enum, Export, ExportItem, Field, Function, FunctionKind, Impl, Item,
    ModuleDecl, Param, Struct, TraitDef, Variant, Visibility, Writeability,
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
                ast::Item::UseDecl(_) => {}
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

    fn lower_trait(&mut self, trait_def: &ast::TraitDef) -> TraitDef {
        TraitDef {
            id: self.source_map.push_trait(syntax_span(trait_def)),
            visibility: if trait_def.is_pub() {
                Visibility::Public
            } else {
                Visibility::Private
            },
            name: trait_def.name_text().unwrap_or_default(),
            methods: Vec::new(),
        }
    }

    fn lower_impl(&mut self, impl_block: &ast::ImplBlock) -> Impl {
        Impl {
            id: self.source_map.push_impl(syntax_span(impl_block)),
            trait_ref: impl_block
                .trait_ref()
                .and_then(|trait_ref| trait_ref.path_text()),
            for_type: impl_block
                .target_type()
                .map(|target_type| self.lower_type(&target_type)),
            methods: Vec::new(),
        }
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
