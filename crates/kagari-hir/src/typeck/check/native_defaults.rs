//! Prove generated forwarding bodies before permitting direct native default calls.

use crate::{
    aggregates::AggregateCatalog,
    declarations::Declarations,
    hir::{expr::ExprKind, ids::ExprId, item::function::Function},
    imports::functions::ImportedFunctions,
    lower::LoweredModule,
    native::NativeBinding,
    resolver::resolved::{ResolvedName, ResolvedNames},
    typeck::{
        FunctionImplementation, TypedFunction,
        table::{CallTarget, TypeTable},
    },
    types::{
        TypeId,
        semantic::{lower_type, raise_type},
    },
};
use kagari_types::callable::NativeDefaultApplication;

/// Borrowed checked facts for one module; no recipe alone establishes a proof.
pub(super) struct NativeDefaultCheck<'a> {
    pub lowered: &'a LoweredModule,
    pub names: &'a ResolvedNames,
    pub declarations: &'a Declarations,
    pub imports: &'a ImportedFunctions,
    pub aggregates: &'a AggregateCatalog,
}

impl NativeDefaultCheck<'_> {
    pub fn forwarding_call(
        &self,
        function: &Function,
        signature: &TypedFunction,
        application: &NativeDefaultApplication,
        table: &TypeTable,
    ) -> Option<ExprId> {
        let block = self.lowered.module.block(function.body?);
        if !block.statements.is_empty() {
            return None;
        }
        let site = block.tail_expr?;
        let ExprKind::Call { args, .. } = &self.lowered.module.expr(site).kind else {
            return None;
        };
        if args.len() != function.params.len()
            || args
                .iter()
                .zip(&function.params)
                .any(|(argument, parameter)| {
                    self.names.expr_resolution(*argument) != Some(ResolvedName::Param(parameter.id))
                })
        {
            return None;
        }
        let call = table.call_resolution(site)?;
        let target = match &call.target {
            CallTarget::Function(id) => {
                if !matches!(
                    self.lowered.native_functions.get(id),
                    Some(NativeBinding::Entry(_))
                ) {
                    return None;
                }
                self.declarations.definition(ResolvedName::Function(*id))?
            }
            CallTarget::SourceFunction(id) => {
                if !matches!(
                    self.imports.target(id)?.signature.implementation,
                    FunctionImplementation::Native(NativeBinding::Entry(_))
                ) {
                    return None;
                }
                id
            }
            _ => return None,
        };
        if target != &application.declaration || call.receiver.is_some() {
            return None;
        }
        let normalize = |ty: &TypeId| {
            let ty = self.aggregates.normalize_type(ty);
            (!ty.is_unresolved()).then(|| lower_type(&ty))
        };
        if call
            .type_arguments
            .iter()
            .map(normalize)
            .collect::<Option<Vec<_>>>()?
            != application
                .arguments
                .iter()
                .map(|ty| normalize(&raise_type(ty)))
                .collect::<Option<Vec<_>>>()?
        {
            return None;
        }
        let applied = call.signature?;
        if applied
            .params
            .iter()
            .map(normalize)
            .collect::<Option<Vec<_>>>()?
            != signature
                .params
                .iter()
                .map(|p| normalize(&p.ty))
                .collect::<Option<Vec<_>>>()?
            || normalize(&applied.return_type)? != normalize(&signature.return_type)?
        {
            return None;
        }
        Some(site)
    }
}
