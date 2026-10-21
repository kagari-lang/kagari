//! Checked applications of noncapturing local declarations as callable values.

use crate::{
    hir::{expr::ExprKind, ids::ExprId, item::function::FunctionKind},
    resolver::resolved::ResolvedName,
    typeck::{BodyTypeEnv, body::BodyChecker, inference, table::CallTarget},
    types::{TypeId, TypeSubstitution},
};

impl BodyChecker<'_> {
    /// Specializes a local function value using its own binders and ordinary bounds.
    /// The application shares checked callable facts with calls; no body executes here.
    pub(super) fn infer_local_function_value(
        &mut self,
        site: ExprId,
        env: &BodyTypeEnv,
        expected: Option<&TypeId>,
    ) -> Option<TypeId> {
        if !matches!(self.lowered.module.expr(site).kind, ExprKind::Name { .. }) {
            return None;
        }
        let ResolvedName::Function(id) = self.names.expr_resolution(site)? else {
            return None;
        };
        if !matches!(
            self.lowered.module.functions[id.index()].kind,
            FunctionKind::Local { .. }
        ) {
            return None;
        }
        let function = self.function_index.by_id.get(&id)?;
        let template = TypeId::Function {
            params: function.params.iter().map(|p| p.ty.clone()).collect(),
            result: Box::new(function.return_type.clone()),
        };
        let mut substitution = TypeSubstitution::default();
        self.seed_explicit_arguments(site, &function.generic_params, &mut substitution);
        if let Some(expected) = expected {
            inference::infer(
                &template,
                expected,
                &function.generic_params,
                &mut substitution,
                self.cancel,
                Some(inference::Context {
                    definitions: self.declarations.definitions(),
                    aggregates: self.aggregates,
                    assumptions: &env.generic_bounds,
                }),
            )
            .ok()?;
        }
        let arguments = self.finish_inferred_arguments(
            &mut substitution,
            &function.generic_params,
            &function.name,
            site,
            false,
        );
        self.check_generic_call_bounds(
            &function.generic_params,
            &function.bounds,
            &substitution,
            env,
            site,
        );
        self.type_table
            .insert_call(site, CallTarget::Function(id), None);
        self.type_table.insert_type_arguments(site, arguments);
        self.record_callable_application(site, function, &substitution);
        Some(template.instantiate(&substitution))
    }
}
