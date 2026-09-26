use super::*;

impl BodyChecker<'_> {
    pub(super) fn infer_standard_constructor(
        &mut self,
        site: ExprId,
        env: &mut BodyTypeEnv,
        expected: Option<&TypeId>,
    ) -> Option<TypeId> {
        let (callee, args) = match &self.lowered.module.expr(site).kind {
            ExprKind::Name { .. } => (site, Vec::new()),
            ExprKind::Call { callee, args } => (*callee, args.to_vec()),
            _ => return None,
        };
        let Some(ResolvedName::StandardVariant(variant)) = self.names.expr_resolution(callee)
        else {
            return None;
        };
        let kind = variant.kind();
        let explicit = match &self.lowered.module.expr(callee).kind {
            ExprKind::Name {
                explicit_type: Some(ty),
                ..
            } => Some(self.resolve_constructor_type(*ty, env)),
            _ => None,
        };
        let inferred_return =
            (!self.closure_returns.is_empty()).then(|| self.expected_return.clone());
        let context = explicit
            .as_ref()
            .or(expected.filter(|ty| **ty != TypeId::Unknown))
            .or(inferred_return.as_ref());
        let mut types = match context {
            Some(TypeId::StandardEnum { kind: actual, args })
                if *actual == kind && args.len() == kind.spec().arity =>
            {
                args.clone()
            }
            _ => vec![TypeId::Unknown; kind.spec().arity],
        };
        let arity = usize::from(variant.payload().is_some());
        if site != callee && arity == 0 {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::InvalidCallTarget {
                    type_name: format!("{variant:?} (unit variant; omit parentheses)"),
                })
                .with_span(self.lowered.source_map.expr_span(site)),
            );
        } else if args.len() != arity || (site == callee && arity != 0) {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::CallArityMismatch {
                    function_name: format!("{variant:?}"),
                    expected: arity,
                    found: args.len(),
                })
                .with_span(self.lowered.source_map.expr_span(site)),
            );
        }
        for (i, arg) in args.iter().enumerate() {
            let index = variant.payload().filter(|_| i == 0);
            let expected = index.map(|index| types[index].clone());
            let actual = self.infer_expr_with_coercion(*arg, env, expected.as_ref());
            if !crate::typeck::completion::expr_can_complete(
                &self.lowered.module,
                self.names,
                *arg,
                self.cancel,
            )
            .unwrap_or(false)
            {
                continue;
            }
            if let Some(index) = index {
                if types[index].conflicts_with(&actual) {
                    self.emit_arg_mismatch(
                        &format!("{variant:?}"),
                        "value",
                        &types[index],
                        &actual,
                        *arg,
                    );
                } else {
                    types[index].recover_from(&actual);
                }
            }
        }
        self.type_table.insert_standard_constructor(site, variant);
        if args.iter().any(|arg| {
            !crate::typeck::completion::expr_can_complete(
                &self.lowered.module,
                self.names,
                *arg,
                self.cancel,
            )
            .unwrap_or(false)
        }) {
            return Some(TypeId::Unknown);
        }
        for (i, ty) in types.iter().enumerate() {
            if ty.is_unresolved() {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::CannotInferGenericArgument {
                        function_name: format!("{variant:?}"),
                        parameter: if i == 0 { "T" } else { "E" }.into(),
                    })
                    .with_span(self.lowered.source_map.expr_span(site)),
                );
            }
        }
        Some(TypeId::StandardEnum { kind, args: types })
    }

    pub(super) fn check_standard_pattern(
        &mut self,
        pattern: crate::hir::PatternId,
        expected: &TypeId,
        env: &mut BodyTypeEnv,
    ) -> bool {
        let (path, fields) = match &self.lowered.module.pattern(pattern).kind {
            PatternKind::EnumVariant { path, fields } => (path.as_str(), fields.clone()),
            PatternKind::Name { name, .. } => (name.as_str(), Vec::new()),
            _ => return false,
        };
        let Some(variant) = self.names.pattern_variants.get(&pattern).copied() else {
            return false;
        };
        let TypeId::StandardEnum { kind, args } = expected else {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                    expected: expected.display_name(),
                    found: path.into(),
                })
                .with_span(self.lowered.source_map.pattern_span(pattern)),
            );
            return true;
        };
        if *kind != variant.kind()
            || args.len() != kind.spec().arity
            || fields.len() != usize::from(variant.payload().is_some())
        {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::PatternTypeMismatch {
                    expected: expected.display_name(),
                    found: path.into(),
                })
                .with_span(self.lowered.source_map.pattern_span(pattern)),
            );
            return true;
        }
        self.type_table.insert_standard_pattern(pattern, variant);
        if let Some(index) = variant.payload() {
            self.check_pattern(fields[0], &args[index], env);
        }
        true
    }
}

impl BodyChecker<'_> {
    pub(super) fn infer_propagation(
        &mut self,
        site: ExprId,
        operand: ExprId,
        env: &mut BodyTypeEnv,
        expected: Option<&TypeId>,
    ) -> TypeId {
        let context = match &self.expected_return {
            TypeId::StandardEnum { kind, args }
                if *kind != surface::StandardEnum::Ordering && args.len() == kind.spec().arity =>
            {
                let mut args = args.clone();
                args[0] = expected.cloned().unwrap_or(TypeId::Unknown);
                Some(TypeId::StandardEnum { kind: *kind, args })
            }
            _ => None,
        };
        let ty = self.infer_expr_type_expected(operand, env, context.as_ref());
        if !crate::typeck::completion::expr_can_complete(
            &self.lowered.module,
            self.names,
            operand,
            self.cancel,
        )
        .unwrap_or(false)
        {
            return TypeId::Unknown;
        }
        let TypeId::StandardEnum { kind, args } = &ty else {
            if !ty.is_unresolved() {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::ReturnTypeMismatch {
                        function_name: "operator ?".into(),
                        expected: "Option<T> or Result<T, E>".into(),
                        found: ty.display_name(),
                    })
                    .with_span(self.lowered.source_map.expr_span(site)),
                );
            }
            return TypeId::Error;
        };
        if args.len() != kind.spec().arity || *kind == surface::StandardEnum::Ordering {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::ReturnTypeMismatch {
                    function_name: "operator ?".into(),
                    expected: "Option<T> or Result<T, E>".into(),
                    found: ty.display_name(),
                })
                .with_span(self.lowered.source_map.expr_span(site)),
            );
            return TypeId::Error;
        }
        let mut residual_args = args.clone();
        residual_args[0] = TypeId::Unknown;
        let residual = TypeId::StandardEnum {
            kind: *kind,
            args: residual_args,
        };
        if self.expected_return == TypeId::Unknown && !self.closure_returns.is_empty() {
            self.expected_return = residual.clone();
        }
        let compatible = matches!(&self.expected_return, TypeId::StandardEnum { kind: target, args: target_args }
            if target == kind && target_args.len() == kind.spec().arity && (*kind == surface::StandardEnum::Option || !target_args[1].conflicts_with(&args[1])));
        if !compatible {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::ReturnTypeMismatch {
                    function_name: self.function_name.into(),
                    expected: self.expected_return.display_name(),
                    found: residual.display_name(),
                })
                .with_span(self.lowered.source_map.expr_span(site)),
            );
        }
        if let Some(returns) = self.closure_returns.last_mut() {
            returns.push(residual);
        }
        args[0].clone()
    }
}
