use super::*;

impl BodyChecker<'_> {
    pub(super) fn prepare_call_type_arguments(&mut self, site: ExprId, env: &BodyTypeEnv) {
        if env.exprs.contains_key(&site) {
            return;
        }
        let ExprKind::Call {
            callee,
            type_args: Some(types),
            ..
        } = &self.lowered.module.expr(site).kind
        else {
            return;
        };
        let callee = *callee;
        let mut arguments = Vec::new();
        for ty in types {
            self.prepare_annotation_holes(*ty);
            let resolved = resolve_type_in(
                &self.lowered.module,
                *ty,
                TypeContext {
                    declarations: self.declarations,
                    generics: &env.generics,
                    self_type: None,
                    implementation: None,
                },
                self.type_table,
                self.cancel,
            );
            if !self.solving && resolved.is_unresolved() {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::UnknownTypeAnnotation {
                        type_name: display_type(&self.lowered.module, *ty),
                    })
                    .with_span(self.lowered.source_map.type_span(*ty)),
                );
            }
            arguments.push(self.aggregates.normalize_type(&resolved));
        }
        self.explicit_arguments.insert(callee, arguments);
        self.used_explicit_arguments.remove(&callee);
    }

    pub(super) fn seed_explicit_arguments(
        &mut self,
        site: ExprId,
        parameters: &[crate::types::GenericParameterType],
        substitution: &mut crate::types::TypeSubstitution,
    ) {
        let Some(arguments) = self.explicit_arguments.get(&site) else {
            return;
        };
        self.used_explicit_arguments.insert(site);
        if arguments.is_empty() || arguments.len() != parameters.len() {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::InvalidCallTarget {
                    type_name: format!(
                        "expected {} type arguments, found {}",
                        parameters.len(),
                        arguments.len()
                    ),
                })
                .with_span(self.lowered.source_map.expr_span(site)),
            );
        }
        substitution.extend(parameters.iter().cloned().zip(arguments.iter().cloned()));
    }

    pub(super) fn check_call_type_arguments_used(&mut self, site: ExprId) {
        let ExprKind::Call {
            callee,
            type_args: Some(_),
            ..
        } = &self.lowered.module.expr(site).kind
        else {
            return;
        };
        if !self.used_explicit_arguments.contains(callee) {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::InvalidCallTarget {
                    type_name: "this call does not accept explicit type arguments".into(),
                })
                .with_span(self.lowered.source_map.expr_span(site)),
            );
        }
    }

    pub(super) fn prepare_annotation_holes(&mut self, ty: crate::hir::TypeRefId) {
        use crate::hir::TypeKind;
        let mut pending = vec![ty];
        while let Some(ty) = pending.pop() {
            if self.cancel.check().is_err() {
                return;
            }
            match &self.lowered.module.type_ref(ty).kind {
                TypeKind::Named(name) if name == "_" => {
                    let inferred = self.solver.annotation_hole(ty);
                    self.type_table.inference_holes.insert(
                        ty,
                        if self.solving {
                            inferred
                        } else {
                            inferred.diagnose_unknowns()
                        },
                    );
                }
                TypeKind::Generic { args, bindings, .. } => {
                    pending.extend(args);
                    pending.extend(bindings.iter().map(|(_, ty)| ty));
                }
                TypeKind::Tuple(items) => pending.extend(items),
                TypeKind::Array(item) => pending.push(*item),
                TypeKind::Function { params, result } => {
                    pending.extend(params);
                    pending.push(*result);
                }
                TypeKind::Projection {
                    arguments,
                    receiver,
                    trait_ref,
                    ..
                } => {
                    pending.extend(arguments);
                    pending.extend([*receiver, *trait_ref]);
                }
                TypeKind::Named(_) => {}
            }
        }
    }

    pub(super) fn constrain_declared_bound(
        &mut self,
        actual: &TypeId,
        interface: &crate::types::NominalType,
    ) {
        if !self.solving {
            return;
        }
        if crate::builtin::traits::StandardTrait::from_id(&interface.declaration)
            .is_some_and(|kind| kind.aggregation())
            && let Some(item) = crate::builtin::traits::numeric_aggregation_item(actual)
            && let [argument] = interface.arguments.as_slice()
        {
            let _ = self.solver.constrain(&item, argument, self.cancel);
        }
        let mut candidates = crate::builtin::declarations::implementations(actual)
            .into_iter()
            .filter(|implementation| {
                implementation.trait_declaration().item.identity() == interface.declaration
            })
            .filter_map(|implementation| {
                let arguments = implementation.arguments(actual)?;
                let declared = crate::types::NominalType {
                    declaration: interface.declaration.clone(),
                    arguments: implementation
                        .trait_arguments
                        .iter()
                        .map(|ty| ty.instantiate(&arguments))
                        .collect(),
                    associated_types: Default::default(),
                };
                (!TypeId::Trait(declared.clone()).conflicts_with(&TypeId::Trait(interface.clone())))
                    .then_some(declared)
            })
            .collect::<Vec<_>>();
        for implementation in self.aggregates.implementations() {
            if self.cancel.check().is_err() {
                return;
            }
            if implementation.trait_type.declaration != interface.declaration {
                continue;
            }
            let mut substitution = crate::types::TypeSubstitution::default();
            if super::super::inference::infer(
                &implementation.for_type,
                actual,
                &implementation.generic_params,
                &mut substitution,
                self.cancel,
            )
            .is_err()
            {
                return;
            }
            let receiver = implementation.for_type.instantiate(&substitution);
            let declared = implementation.trait_type.instantiate(&substitution);
            if !receiver.conflicts_with(actual)
                && !TypeId::Trait(declared.clone())
                    .conflicts_with(&TypeId::Trait(interface.clone()))
            {
                candidates.push(declared);
            }
        }
        if let [declared] = candidates.as_slice() {
            let _ = self.solver.constrain(
                &TypeId::Trait(declared.clone()),
                &TypeId::Trait(interface.clone()),
                self.cancel,
            );
        }
    }

    /// Revisit deferred semantic obligations until substitutions stop changing.
    /// Only the final checked pass publishes diagnostics and semantic facts.
    pub(crate) fn solve_body(
        &mut self,
        block: BlockId,
        env: &mut BodyTypeEnv,
        expected: Option<&TypeId>,
    ) -> TypeId {
        let initial_env = env.clone();
        let initial_table = self.type_table.clone();
        let diagnostic_start = self.diagnostics.len();
        self.body_inference = true;
        self.solving = true;
        const MAX_ROUNDS: usize = 128;
        let mut converged = false;
        for _ in 0..MAX_ROUNDS {
            if self.cancel.check().is_err() {
                return TypeId::Unknown;
            }
            let revision = self.solver.revision;
            *env = initial_env.clone();
            *self.type_table = initial_table.clone();
            self.diagnostics.truncate(diagnostic_start);
            self.propagation_defaults.clear();
            self.infer_block_types_expected(block, env, expected);
            if self.solver.revision == revision {
                if self.solver.apply_numeric_defaults() {
                    continue;
                }
                // The enclosing error type is a fallback, never an equality
                // constraint on an independently inferred source error.
                for (source, target) in &self.propagation_defaults {
                    if let TypeId::Inference(_) = self.solver.resolve(source) {
                        let _ = self.solver.constrain(source, target, self.cancel);
                    }
                }
                if self.solver.revision != revision {
                    continue;
                }
                converged = true;
                break;
            }
        }
        *env = initial_env;
        *self.type_table = initial_table;
        self.diagnostics.truncate(diagnostic_start);
        self.solving = false;
        if !converged {
            self.diagnostics
                .push(Diagnostic::error(DiagnosticKind::CompileLimitExceeded {
                    resource: "body inference rounds",
                    limit: MAX_ROUNDS,
                }));
        }
        let result = self.infer_block_types_expected(block, env, expected);
        self.type_table.inference_holes.clear();
        result
    }

    pub(super) fn inference_variable(&mut self, site: ExprId, slot: usize) -> TypeId {
        if !self.body_inference {
            return TypeId::Unknown;
        }
        let variable = self.solver.variable(site, slot);
        self.solver.resolve(&variable)
    }

    pub(super) fn expression_context(
        &mut self,
        site: ExprId,
        expected: Option<&TypeId>,
    ) -> Option<TypeId> {
        if !self.body_inference {
            return expected.cloned();
        }
        let variable = self.solver.variable(site, 0);
        if let Some(expected) = expected {
            let _ = self.solver.constrain(&variable, expected, self.cancel);
            return Some(self.solver.resolve(expected));
        }
        let inferred = self.solver.resolve(&variable);
        (!matches!(inferred, TypeId::Inference(_))).then_some(inferred)
    }

    pub(super) fn constrain_expression(
        &mut self,
        site: ExprId,
        ty: TypeId,
        expected: Option<&TypeId>,
    ) -> TypeId {
        if !self.body_inference {
            return ty;
        }
        let completes = super::super::completion::expr_can_complete(
            &self.lowered.module,
            self.names,
            site,
            self.cancel,
        )
        .unwrap_or(false);
        if completes {
            let variable = self.solver.variable(site, 0);
            let _ = self.solver.constrain(&variable, &ty, self.cancel);
            if let Some(expected) = expected {
                let _ = self.solver.constrain(expected, &ty, self.cancel);
            }
        }
        let resolved = self.solver.resolve(&ty);
        if !self.solving {
            let mut pending = false;
            resolved.substitute_once(|ty| {
                pending |= matches!(ty, TypeId::Inference(_));
                None
            });
            if pending && completes {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::CannotInferGenericArgument {
                        function_name: "expression".into(),
                        parameter: "type (add an annotation)".into(),
                    })
                    .with_span(self.lowered.source_map.expr_span(site)),
                );
                return resolved.diagnose_unknowns();
            }
            if pending {
                return resolved.diagnose_unknowns();
            }
        }
        resolved
    }
}
