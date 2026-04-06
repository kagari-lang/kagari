use kagari_common::{Diagnostic, DiagnosticKind, TypePosition};
use smallvec::SmallVec;
use std::collections::HashMap;

use crate::{
    BoxedDiagnosticBuffer,
    hir::FunctionKind,
    hir::{BinaryOp, ConstId, ConstItem, ExprId, ExprKind, PrefixOp},
    lower::LoweredModule,
    resolver::{ResolvedName, ResolvedNames},
    typeck::body::BodyChecker,
    typeck::ty::{display_type, display_type_id, resolve_type},
    typeck::{
        BodyTypeEnv, FunctionTypeIndex, TopLevelTypeIndex, TypeIndexes, TypeTable, TypedFunction,
        TypedFunctionBuffer, TypedModule, TypedParameter, TypedParameterBuffer, TypedStatic,
    },
    types::{BuiltinType, TypeId},
};

pub fn check_module(
    lowered: &LoweredModule,
    names: &ResolvedNames,
) -> Result<TypedModule, BoxedDiagnosticBuffer> {
    let mut diagnostics = SmallVec::<[Diagnostic; 4]>::new();
    let mut functions: TypedFunctionBuffer = SmallVec::new();
    let mut function_index = FunctionTypeIndex::default();
    let mut top_level_index = TopLevelTypeIndex::default();

    for function in &lowered.module.functions {
        let mut params: TypedParameterBuffer = SmallVec::new();
        let function_name = if function.name.is_empty() {
            "<missing>".to_string()
        } else {
            function.name.clone()
        };

        for param in &function.params {
            let param_name = if param.name.is_empty() {
                "<missing>".to_string()
            } else {
                param.name.clone()
            };
            let param_ty_name = display_type(&lowered.module, param.ty);

            match resolve_type(&lowered.module, param.ty) {
                Some(ty) => {
                    params.push(TypedParameter {
                        id: param.id,
                        name: param_name,
                        ty,
                    });
                }
                None => diagnostics.push(
                    Diagnostic::error(DiagnosticKind::UnknownType {
                        type_name: param_ty_name,
                        function_name: function_name.clone(),
                        position: TypePosition::Parameter,
                    })
                    .with_span(lowered.source_map.type_span(param.ty)),
                ),
            }
        }

        let return_type = match &function.return_type {
            Some(ty_ref) => match resolve_type(&lowered.module, *ty_ref) {
                Some(ty) => ty,
                None => {
                    let ty_name = display_type(&lowered.module, *ty_ref);
                    diagnostics.push(
                        Diagnostic::error(DiagnosticKind::UnknownType {
                            type_name: ty_name,
                            function_name: function_name.clone(),
                            position: TypePosition::Return,
                        })
                        .with_span(lowered.source_map.type_span(*ty_ref)),
                    );
                    TypeId::Builtin(BuiltinType::Unit)
                }
            },
            None => TypeId::Builtin(BuiltinType::Unit),
        };

        let typed_function = TypedFunction {
            id: function.id,
            name: function_name,
            params,
            return_type,
        };
        function_index
            .by_id
            .insert(function.id, typed_function.clone());
        functions.push(typed_function);
    }

    if !diagnostics.is_empty() {
        Err(Box::new(diagnostics))
    } else {
        let mut type_table = TypeTable::default();
        for const_item in &lowered.module.consts {
            let ty = match const_item.ty {
                Some(ty_ref) => match resolve_type(&lowered.module, ty_ref) {
                    Some(ty) => ty,
                    None => {
                        diagnostics.push(
                            Diagnostic::error(DiagnosticKind::UnknownConstType {
                                const_name: const_item.name.clone(),
                                type_name: display_type(&lowered.module, ty_ref),
                            })
                            .with_span(lowered.source_map.const_span(const_item.id)),
                        );
                        TypeId::Builtin(BuiltinType::Unit)
                    }
                },
                None => {
                    let mut env = BodyTypeEnv::default();
                    let mut checker = BodyChecker::new(
                        lowered,
                        names,
                        TypeIndexes {
                            function_index: &function_index,
                            top_level_index: &top_level_index,
                        },
                        &mut diagnostics,
                        &mut type_table,
                        "<const>",
                        TypeId::Builtin(BuiltinType::Unit),
                    );
                    checker.infer_expr_type(const_item.initializer, &mut env)
                }
            };
            if const_item.ty.is_some() {
                let mut env = BodyTypeEnv::default();
                let mut checker = BodyChecker::new(
                    lowered,
                    names,
                    TypeIndexes {
                        function_index: &function_index,
                        top_level_index: &top_level_index,
                    },
                    &mut diagnostics,
                    &mut type_table,
                    "<const>",
                    TypeId::Builtin(BuiltinType::Unit),
                );
                let _ = checker.infer_expr_type(const_item.initializer, &mut env);
            }
            top_level_index.consts.insert(const_item.id, ty.clone());
        }

        validate_const_initializers(
            lowered,
            names,
            &top_level_index,
            &type_table,
            &mut diagnostics,
        );

        for static_item in &lowered.module.statics {
            let ty = match static_item.ty {
                Some(ty_ref) => match resolve_type(&lowered.module, ty_ref) {
                    Some(ty) => ty,
                    None => {
                        diagnostics.push(
                            Diagnostic::error(DiagnosticKind::UnknownStaticType {
                                static_name: static_item.name.clone(),
                                type_name: display_type(&lowered.module, ty_ref),
                            })
                            .with_span(lowered.source_map.static_span(static_item.id)),
                        );
                        TypeId::Builtin(BuiltinType::Unit)
                    }
                },
                None => {
                    let mut env = BodyTypeEnv::default();
                    let mut checker = BodyChecker::new(
                        lowered,
                        names,
                        TypeIndexes {
                            function_index: &function_index,
                            top_level_index: &top_level_index,
                        },
                        &mut diagnostics,
                        &mut type_table,
                        "<static>",
                        TypeId::Builtin(BuiltinType::Unit),
                    );
                    checker.infer_expr_type(static_item.initializer, &mut env)
                }
            };
            top_level_index.statics.insert(
                static_item.id,
                TypedStatic {
                    ty,
                    mutable: static_item.mutable,
                },
            );
        }

        for function in &lowered.module.functions {
            let mut env = BodyTypeEnv::default();
            if let Some(typed_function) = function_index.by_id.get(&function.id) {
                for param in &typed_function.params {
                    env.params.insert(param.id, param.ty.clone());
                }
                let mut checker = BodyChecker::new(
                    lowered,
                    names,
                    TypeIndexes {
                        function_index: &function_index,
                        top_level_index: &top_level_index,
                    },
                    &mut diagnostics,
                    &mut type_table,
                    &typed_function.name,
                    typed_function.return_type.clone(),
                );
                let body_ty = checker.infer_block_types(function.body, &mut env);
                if matches!(function.kind, FunctionKind::ModuleInit) {
                    if let Some(indexed) = function_index.by_id.get_mut(&function.id) {
                        indexed.return_type = body_ty.clone();
                    }
                    for typed in &mut functions {
                        if typed.id == function.id {
                            typed.return_type = body_ty.clone();
                            break;
                        }
                    }
                    continue;
                }
                if body_ty != typed_function.return_type {
                    diagnostics.push(
                        Diagnostic::error(DiagnosticKind::ReturnTypeMismatch {
                            function_name: typed_function.name.clone(),
                            expected: display_type_id(&typed_function.return_type),
                            found: display_type_id(&body_ty),
                        })
                        .with_span(lowered.source_map.function_span(function.id)),
                    );
                }
            }
        }

        if diagnostics.is_empty() {
            Ok(TypedModule {
                functions,
                consts: top_level_index.consts,
                statics: top_level_index.statics,
                type_table,
            })
        } else {
            Err(Box::new(diagnostics))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConstVisitState {
    Visiting,
    Done,
}

fn validate_const_initializers(
    lowered: &LoweredModule,
    names: &ResolvedNames,
    top_level_index: &TopLevelTypeIndex,
    type_table: &TypeTable,
    diagnostics: &mut SmallVec<[Diagnostic; 4]>,
) {
    struct ConstValidator<'a> {
        lowered: &'a LoweredModule,
        names: &'a ResolvedNames,
        top_level_index: &'a TopLevelTypeIndex,
        type_table: &'a TypeTable,
        diagnostics: &'a mut SmallVec<[Diagnostic; 4]>,
        states: HashMap<ConstId, ConstVisitState>,
    }

    impl ConstValidator<'_> {
        fn validate_const(&mut self, const_id: ConstId) {
            match self.states.get(&const_id) {
                Some(ConstVisitState::Done) => return,
                Some(ConstVisitState::Visiting) => {
                    let const_item = self.const_item(const_id);
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::ConstCycle {
                            const_name: const_item.name.clone(),
                        })
                        .with_span(self.lowered.source_map.const_span(const_id)),
                    );
                    return;
                }
                None => {}
            }

            self.states.insert(const_id, ConstVisitState::Visiting);
            let const_item = self.const_item(const_id);
            if let Some(const_ty) = self.top_level_index.consts.get(&const_id)
                && !supports_const_type(const_ty)
            {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::InvalidConstInitializer {
                        const_name: const_item.name.clone(),
                        reason: format!(
                            "const type `{}` is heap-backed; const supports value types only",
                            display_type_id(const_ty)
                        ),
                    })
                    .with_span(self.lowered.source_map.const_span(const_id)),
                );
                self.states.insert(const_id, ConstVisitState::Done);
                return;
            }

            self.validate_const_expr(const_item.id, const_item.initializer);
            self.states.insert(const_id, ConstVisitState::Done);
        }

        fn validate_const_expr(&mut self, owner: ConstId, expr_id: ExprId) {
            let expr = self.lowered.module.expr(expr_id);
            match &expr.kind {
                ExprKind::Literal(_) => {}
                ExprKind::Name(_) => {
                    let Some(resolved) = self.names.expr_resolution(expr_id) else {
                        self.emit_invalid_const(
                            owner,
                            expr_id,
                            "const initializer must use literals or other consts",
                        );
                        return;
                    };

                    match resolved {
                        ResolvedName::Const(id) => self.validate_const(id),
                        _ => self.emit_invalid_const(
                            owner,
                            expr_id,
                            "const initializer must use literals or other consts",
                        ),
                    }
                }
                ExprKind::Prefix { op, expr } => {
                    self.validate_const_expr(owner, *expr);

                    let Some(expr_ty) = self.type_table.expr_type(*expr) else {
                        self.emit_invalid_const(
                            owner,
                            expr_id,
                            "const initializer has unknown operand type",
                        );
                        return;
                    };

                    let supported = matches!(
                        (op, expr_ty),
                        (
                            PrefixOp::Neg,
                            TypeId::Builtin(BuiltinType::I32 | BuiltinType::F32)
                        ) | (PrefixOp::Not, TypeId::Builtin(BuiltinType::Bool))
                    );
                    if !supported {
                        self.emit_invalid_const(
                            owner,
                            expr_id,
                            "unsupported unary const expression",
                        );
                    }
                }
                ExprKind::Binary { lhs, op, rhs } => {
                    self.validate_const_expr(owner, *lhs);
                    self.validate_const_expr(owner, *rhs);

                    let lhs_ty = self.type_table.expr_type(*lhs);
                    let rhs_ty = self.type_table.expr_type(*rhs);
                    if !supports_const_binary(op, lhs_ty.as_ref(), rhs_ty.as_ref()) {
                        self.emit_invalid_const(
                            owner,
                            expr_id,
                            "unsupported binary const expression",
                        );
                    }
                }
                ExprKind::Tuple(elements) | ExprKind::Array(elements) => {
                    for element in elements {
                        self.validate_const_expr(owner, *element);
                    }
                }
                ExprKind::StructInit { fields, .. } => {
                    for field in fields {
                        self.validate_const_expr(owner, field.value);
                    }
                }
                _ => self.emit_invalid_const(
                    owner,
                    expr_id,
                    "unsupported const initializer expression",
                ),
            }

            if let Some(resolved) = self.names.expr_resolution(expr_id)
                && let ResolvedName::Const(id) = resolved
                && !self.top_level_index.consts.contains_key(&id)
            {
                self.emit_invalid_const(
                    owner,
                    expr_id,
                    "const initializer references an unresolved const type",
                );
            }
        }

        fn emit_invalid_const(&mut self, owner: ConstId, expr_id: ExprId, reason: &'static str) {
            let const_item = self.const_item(owner);
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::InvalidConstInitializer {
                    const_name: const_item.name.clone(),
                    reason: reason.to_owned(),
                })
                .with_span(self.lowered.source_map.expr_span(expr_id)),
            );
        }

        fn const_item(&self, const_id: ConstId) -> &ConstItem {
            self.lowered
                .module
                .consts
                .iter()
                .find(|item| item.id == const_id)
                .expect("const id should exist")
        }
    }

    fn supports_const_binary(op: &BinaryOp, lhs: Option<&TypeId>, rhs: Option<&TypeId>) -> bool {
        match (op, lhs, rhs) {
            (
                BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div,
                Some(TypeId::Builtin(BuiltinType::I32)),
                Some(TypeId::Builtin(BuiltinType::I32)),
            ) => true,
            (
                BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div,
                Some(TypeId::Builtin(BuiltinType::F32)),
                Some(TypeId::Builtin(BuiltinType::F32)),
            ) => true,
            (BinaryOp::Eq | BinaryOp::NotEq, Some(lhs), Some(rhs)) => lhs == rhs,
            (
                BinaryOp::Lt | BinaryOp::Gt | BinaryOp::Le | BinaryOp::Ge,
                Some(TypeId::Builtin(BuiltinType::I32)),
                Some(TypeId::Builtin(BuiltinType::I32)),
            ) => true,
            (
                BinaryOp::Lt | BinaryOp::Gt | BinaryOp::Le | BinaryOp::Ge,
                Some(TypeId::Builtin(BuiltinType::F32)),
                Some(TypeId::Builtin(BuiltinType::F32)),
            ) => true,
            (
                BinaryOp::AndAnd | BinaryOp::OrOr,
                Some(TypeId::Builtin(BuiltinType::Bool)),
                Some(TypeId::Builtin(BuiltinType::Bool)),
            ) => true,
            _ => false,
        }
    }

    fn supports_const_type(ty: &TypeId) -> bool {
        matches!(ty, TypeId::Builtin(_))
    }

    let mut validator = ConstValidator {
        lowered,
        names,
        top_level_index,
        type_table,
        diagnostics,
        states: HashMap::new(),
    };
    for const_item in &lowered.module.consts {
        validator.validate_const(const_item.id);
    }
}
