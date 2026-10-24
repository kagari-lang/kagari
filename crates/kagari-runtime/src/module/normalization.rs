//! Immutable identity references and destination-local normalization.
use crate::{
    error::RuntimeError, module::VerifiedProgram, reload::updates::coverage::CoverageFacts,
};
use kagari_bytecode::{module::BytecodeModule, program::identity};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        map::DefinitionContext,
        mapping::{DefinitionMapper, DefinitionMappingError},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};
use kagari_contract::{exports::ProgramExports, updates::UpdateEvidence};
use std::{collections::HashSet, sync::Arc};

impl VerifiedProgram {
    /// Retain the exact referenced IDs alongside their immutable supplying records.
    /// Destination scope import and all linked validation still run for every load.
    pub(super) fn prepare_definition_references(
        modules: &[Arc<BytecodeModule>],
        exports: &ProgramExports,
        update_evidence: &UpdateEvidence,
        update_coverage: &CoverageFacts,
    ) -> Result<Arc<[DefinitionId]>, RuntimeError> {
        let cancel = CancellationToken::default();
        let mut ids = HashSet::new();
        for module in modules {
            module
                .visit_definition_ids(
                    &mut |id| {
                        ids.insert(*id);
                        Ok(())
                    },
                    &cancel,
                )
                .map_err(|error| RuntimeError::module_validation(error.to_string()))?;
        }
        exports
            .visit_definition_ids(
                &mut |id| {
                    ids.insert(*id);
                    Ok(())
                },
                &cancel,
            )
            .map_err(|error| RuntimeError::module_validation(error.to_string()))?;
        let mut visit = |id: &DefinitionId| {
            ids.insert(*id);
            Ok(())
        };
        update_evidence
            .visit_definition_ids(&mut visit, &cancel)
            .map_err(|error| RuntimeError::module_validation(error.to_string()))?;
        update_coverage
            .visit_definition_ids(&mut visit, &cancel)
            .map_err(|error| RuntimeError::module_validation(error.to_string()))?;
        Ok(ids.into_iter().collect::<Vec<_>>().into())
    }

    /// Import exact identities once while retaining the immutable source version.
    /// Remapping changes only scoped identity references; no verification flag or hash
    /// can create this evidence, and the original sealed records stay immutable.
    pub(crate) fn normalized(&self, context: &DefinitionContext) -> Result<Self, RuntimeError> {
        if self.definitions.id() == context.snapshot().id() {
            return Ok(self.clone());
        }
        let cancel = CancellationToken::default();
        let (
            modules,
            exports,
            export_targets,
            definition_references,
            update_evidence,
            update_coverage,
        ) = if self.definition_references.is_empty() {
            (
                self.modules.clone(),
                self.exports.clone(),
                self.export_targets.clone(),
                self.definition_references.clone(),
                self.update_evidence.clone(),
                self.update_coverage.clone(),
            )
        } else {
            let remap = context
                .import(
                    &self.definitions,
                    self.definition_references.iter().copied(),
                )
                .map_err(|error| RuntimeError::module_validation(error.to_string()))?;
            let modules = self
                .modules
                .iter()
                .map(|module| {
                    module
                        .remap_definitions(&mut DefinitionMapper::new(
                            &mut |id| remap.map(*id).map_err(DefinitionMappingError::from),
                            &context.snapshot(),
                            &cancel,
                        ))
                        .map(Arc::new)
                })
                .collect::<Result<_, _>>()
                .map_err(|error| RuntimeError::module_validation(error.to_string()))?;
            let exports = self
                .exports
                .remap_definitions(&mut DefinitionMapper::new(
                    &mut |id| remap.map(*id).map_err(DefinitionMappingError::from),
                    &context.snapshot(),
                    &cancel,
                ))
                .map_err(|error| RuntimeError::module_validation(error.to_string()))?;
            let export_targets = self
                .export_targets
                .iter()
                .map(|(id, target)| remap.map(*id).map(|id| (id, *target)))
                .collect::<Result<_, _>>()
                .map_err(|error| RuntimeError::module_validation(error.to_string()))?;
            let definition_references = self
                .definition_references
                .iter()
                .map(|id| remap.map(*id))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| RuntimeError::module_validation(error.to_string()))?;
            let mut convert =
                |id: &DefinitionId| remap.map(*id).map_err(DefinitionMappingError::from);
            let target = context.snapshot();
            let mut mapper = DefinitionMapper::new(&mut convert, &target, &cancel);
            let update_evidence = self
                .update_evidence
                .remap_definitions(&mut mapper)
                .map_err(|error| RuntimeError::module_validation(error.to_string()))?;
            let update_coverage = self
                .update_coverage
                .remap_definitions(&mut mapper)
                .map_err(|error| RuntimeError::module_validation(error.to_string()))?;
            (
                modules,
                Arc::new(exports),
                Arc::new(export_targets),
                definition_references.into(),
                Arc::new(update_evidence),
                Arc::new(update_coverage),
            )
        };
        let definitions = identity::collect(
            &modules.iter().map(Arc::as_ref).collect::<Vec<_>>(),
            context,
            &Default::default(),
        )
        .map_err(|error| RuntimeError::module_validation(error.to_string()))?;
        Ok(Self {
            update_evidence,
            update_coverage,
            initializers: self.initializers.clone(),
            root: self.root,
            exports,
            export_targets,
            modules,
            definitions,
            definition_references,
            execution: self.execution.clone(),
            version: self.version.clone(),
            dependencies: self.dependencies.clone(),
        })
    }
}
