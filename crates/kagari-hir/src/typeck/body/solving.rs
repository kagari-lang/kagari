use super::*;

impl BodyChecker<'_> {
    pub(super) fn constrain_declared_bound(
        &mut self,
        actual: &TypeId,
        interface: &crate::types::NominalType,
    ) {
        if !self.solving {
            return;
        }
        let candidates = crate::builtin::declarations::implementations(actual)
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
            self.infer_block_types_expected(block, env, expected);
            if self.solver.revision == revision {
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
        self.infer_block_types_expected(block, env, expected)
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
