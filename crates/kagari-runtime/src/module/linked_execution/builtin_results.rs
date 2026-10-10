//! Closed builtin result contracts belong to the exact installed program member.
use crate::{
    Runtime, error::RuntimeError, frame::types::arguments::TypeArgument, module::LoadedModule,
};
use kagari_types::language::binding;

#[derive(Clone, Copy)]
pub(crate) enum BuiltinResult {
    Ordering,
    OptionalOrdering,
}

/// Fixed language contracts, not an unbounded cache of names or call sites.
/// These type facts retain immutable provenance, never executable leases or values.
#[derive(Debug, Default)]
pub(super) struct BuiltinResults {
    ordering: Option<TypeArgument>,
    optional_ordering: Option<TypeArgument>,
}

impl BuiltinResults {
    fn get(&self, result: BuiltinResult) -> &Option<TypeArgument> {
        match result {
            BuiltinResult::Ordering => &self.ordering,
            BuiltinResult::OptionalOrdering => &self.optional_ordering,
        }
    }

    fn get_mut(&mut self, result: BuiltinResult) -> &mut Option<TypeArgument> {
        match result {
            BuiltinResult::Ordering => &mut self.ordering,
            BuiltinResult::OptionalOrdering => &mut self.optional_ordering,
        }
    }
}

impl Runtime {
    pub(crate) fn builtin_result_type(
        &self,
        owner: &LoadedModule,
        result: BuiltinResult,
    ) -> Result<TypeArgument, RuntimeError> {
        self.validate_loaded_module(owner)?;
        let invalid = || RuntimeError::module_validation("invalid builtin result owner");
        {
            let records = self.modules.inner.try_borrow().map_err(|_| invalid())?;
            let execution = records
                .resolve(owner)
                .and_then(|record| record.execution.as_ref())
                .ok_or_else(invalid)?;
            if let Some(applied) = execution
                .builtin_results
                .as_deref()
                .and_then(|results| results.get(result).as_ref())
            {
                return Ok(applied.clone());
            }
        }
        // Preserve builtin failure order: prepare only after operand validation,
        // and only the result contract actually requested by the operation.
        // No module-store borrow crosses type/layout preparation.
        let ty = match result {
            BuiltinResult::Ordering => binding::ordering(),
            BuiltinResult::OptionalOrdering => binding::option(binding::ordering()),
        };
        let applied = self.portable_type_argument(owner, &ty)?;
        let mut records = self.modules.inner.try_borrow_mut().map_err(|_| invalid())?;
        let execution = records
            .resolve_mut(owner)
            .and_then(|record| record.execution.as_mut())
            .ok_or_else(invalid)?;
        Ok(execution
            .builtin_results
            .get_or_insert_with(Default::default)
            .get_mut(result)
            .get_or_insert(applied)
            .clone())
    }
}
