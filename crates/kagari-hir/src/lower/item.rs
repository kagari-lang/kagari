use kagari_syntax::ast;

use crate::hir::{
    BlockData, ConstItem, Enum, Export, ExportItem, Field, Function, FunctionKind, Item, Param,
    StaticItem, Struct, Variant, Visibility,
};
use crate::lower::context::{Lowerer, syntax_span};

impl Lowerer {
    pub(crate) fn lower_module(&mut self, module: &ast::SourceFile) {
        for item in module.items() {
            match item {
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
                ast::Item::StaticDef(static_def) => {
                    let hir_static = self.lower_static(&static_def);
                    if hir_static.visibility == Visibility::Public {
                        self.module.exports.push(Export {
                            name: hir_static.name.clone(),
                            item: ExportItem::Static(hir_static.id),
                        });
                    }
                    self.module.items.push(Item::Static(hir_static.id));
                    self.module.statics.push(hir_static);
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

    fn lower_static(&mut self, static_def: &ast::StaticDef) -> StaticItem {
        StaticItem {
            id: self.source_map.push_static(syntax_span(static_def)),
            visibility: if static_def.is_pub() {
                Visibility::Public
            } else {
                Visibility::Private
            },
            mutable: static_def.is_mut(),
            name: static_def.name_text().unwrap_or_default(),
            ty: static_def.ty().map(|ty| self.lower_type(&ty)),
            initializer: static_def
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
        }
    }
}
