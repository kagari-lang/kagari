//! Complete referenced identity inventory, prepared once per immutable product.
use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    module::VerifiedProgram,
    reload::updates::coverage::CoverageFacts,
};
use kagari_bytecode::{module::BytecodeModule, source_package::VerifiedSourcePackage};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::DefinitionMappingError, scoped::DefinitionReferences, table::DefinitionId,
    },
};
use kagari_contract::{exports::ProgramExports, updates::UpdateEvidence};
use std::{collections::HashSet, sync::Arc};

impl VerifiedProgram {
    /// Source-only declarations participate in the same import/publication inventory
    /// as executable references. The publication owner also includes their ancestors.
    pub(super) fn prepare_definition_references(
        modules: &[Arc<BytecodeModule>],
        exports: &ProgramExports,
        update_evidence: &UpdateEvidence,
        update_coverage: &CoverageFacts,
        source: Option<&VerifiedSourcePackage>,
        cancel: &CancellationToken,
    ) -> Result<Arc<[DefinitionId]>, RuntimeError> {
        let mut ids = HashSet::new();
        let mut visit = |id: &DefinitionId| {
            ids.insert(*id);
            Ok(())
        };
        for module in modules {
            module
                .visit_definition_ids(&mut visit, cancel)
                .map_err(identity_error)?;
        }
        exports
            .visit_definition_ids(&mut visit, cancel)
            .map_err(identity_error)?;
        update_evidence
            .visit_definition_ids(&mut visit, cancel)
            .map_err(identity_error)?;
        update_coverage
            .visit_definition_ids(&mut visit, cancel)
            .map_err(identity_error)?;
        if let Some(source) = source {
            source
                .records()
                .visit_definition_ids(&mut visit, cancel)
                .map_err(identity_error)?;
        }
        cancel
            .check()
            .map_err(|_| identity_error(DefinitionMappingError::Cancelled))?;
        Ok(ids.into_iter().collect::<Vec<_>>().into())
    }
}

fn identity_error(error: DefinitionMappingError) -> RuntimeError {
    match error {
        DefinitionMappingError::Cancelled => RuntimeError::new(
            RuntimeErrorKind::Cancelled,
            "program identity import cancelled",
        ),
        error => RuntimeError::module_validation(error.to_string()),
    }
}
