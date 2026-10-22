//! Shared capture checks and callable boundaries for closures and async blocks.

use crate::{
    hir::{
        expr::{ClosureParam, ExprKind},
        ids::ExprId,
    },
    resolver::resolved::ResolvedName,
    typeck::{
        BodyTypeEnv, asynchronous as async_types,
        body::BodyChecker,
        completion,
        ty::{TypeContext, display_type_id, resolve_type_in},
    },
    types::TypeId,
};
use kagari_source::diagnostic::{Diagnostic, DiagnosticKind};
use kagari_types::scalar::BuiltinType;
use std::mem;

impl BodyChecker<'_> {
    pub(super) fn infer_closure(
        &mut self,
        expr_id: ExprId,
        env: &mut BodyTypeEnv,
        expected: Option<&TypeId>,
    ) -> TypeId {
        let (params, body, is_async, is_block): (&[ClosureParam], _, _, _) =
            match &self.lowered.module.expr(expr_id).kind {
                ExprKind::Closure {
                    params,
                    body,
                    is_async,
                } => (params, *body, *is_async, false),
                ExprKind::AsyncBlock { body } => (&[], *body, true, true),
                _ => unreachable!("capturing body"),
            };
        let name = if is_block { "async block" } else { "closure" };
        for capture in self.names.closure_captures(expr_id) {
            let captured = match capture {
                ResolvedName::Local(id) => env.locals.get(id),
                ResolvedName::Param(id) => env.params.get(id),
                _ => None,
            };
            if let Some(ty) = captured
                && ty.contains_host_value()
            {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::InvalidClosureCapture {
                        type_name: display_type_id(self.declarations.definitions(), ty),
                    })
                    .with_span(self.lowered.source_map.expr_span(expr_id)),
                );
            }
        }
        let expected_params = match expected.filter(|_| !is_block) {
            Some(TypeId::Function { params, .. }) => Some(params.as_slice()),
            _ => None,
        };
        let expected_result = match expected {
            expected if is_block => expected,
            Some(TypeId::Function { result, .. }) => Some(result.as_ref()),
            _ => None,
        };
        let expected_result = if is_async {
            expected_result.and_then(|ty| async_types::future_output(self.declarations, ty))
        } else {
            expected_result
        };
        let mut closure_env = env.clone();
        let mut param_types = Vec::with_capacity(params.len());
        for (index, param) in params.iter().enumerate() {
            let ty = if let Some(annotation) = param.ty {
                self.prepare_annotation_holes(annotation);
                resolve_type_in(
                    &self.lowered.module,
                    annotation,
                    TypeContext {
                        declarations: self.declarations,
                        generics: &env.generics,
                        self_type: None,
                        implementation: None,
                    },
                    self.type_table,
                    self.cancel,
                )
            } else if let Some(ty) = expected_params.and_then(|types| types.get(index)) {
                ty.clone()
            } else if self.body_inference {
                self.inference_variable(expr_id, 128 + index)
            } else {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::ExpectedType)
                        .with_span(self.lowered.source_map.local_span(param.local)),
                );
                TypeId::Unknown
            };
            self.type_table.insert_local(param.local, ty.clone());
            if is_async && ty.contains_host_value() {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::InvalidAsyncCapture {
                        type_name: display_type_id(self.declarations.definitions(), &ty),
                    })
                    .with_span(self.lowered.source_map.local_span(param.local)),
                );
            }
            closure_env.locals.insert(param.local, ty.clone());
            param_types.push(ty);
        }
        let old_async = mem::replace(&mut self.async_body, is_async);
        let old_return = mem::replace(
            &mut self.expected_return,
            expected_result.cloned().unwrap_or(TypeId::Unknown),
        );
        let old_loop_depth = mem::replace(&mut self.loop_depth, 0);
        let old_loop_results = mem::take(&mut self.loop_results);
        let old_name = mem::replace(&mut self.function_name, name);
        self.closure_returns.push(Vec::new());
        let body_result = self.infer_expr_with_coercion(body, &mut closure_env, expected_result);
        let returns = self.closure_returns.pop().expect("closure return context");
        let completes = completion::expr_can_complete(
            &self.lowered.module,
            self.names,
            self.type_table,
            body,
            self.cancel,
        )
        .unwrap_or(false);
        let mut result = if completes {
            body_result
        } else {
            expected_result
                .filter(|ty| !matches!(ty, TypeId::Unknown))
                .cloned()
                .or_else(|| returns.first().cloned())
                .unwrap_or_else(|| self.inference_variable(expr_id, 2))
        };
        if !completes && returns.is_empty() {
            self.solver.defer_never(&result);
            if matches!(result, TypeId::Unknown) {
                result = TypeId::Builtin(BuiltinType::Never);
            }
        }
        for returned in returns {
            if result.conflicts_with(&returned) {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::ReturnTypeMismatch {
                        function_name: name.into(),
                        expected: display_type_id(self.declarations.definitions(), &result),
                        found: display_type_id(self.declarations.definitions(), &returned),
                    })
                    .with_span(self.lowered.source_map.expr_span(body)),
                );
            } else {
                result.recover_from(&returned);
            }
        }
        self.async_body = old_async;
        self.function_name = old_name;
        self.expected_return = old_return;
        self.loop_depth = old_loop_depth;
        self.loop_results = old_loop_results;
        if is_async {
            result = async_types::future_type(self.declarations, result).unwrap_or_else(|| {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::UnsupportedSyntax {
                        feature: "async requires one installed Future storage declaration",
                    })
                    .with_span(self.lowered.source_map.expr_span(expr_id)),
                );
                TypeId::Error
            });
        }
        if is_block {
            result
        } else {
            TypeId::Function {
                params: param_types,
                result: Box::new(result),
            }
        }
    }
}
