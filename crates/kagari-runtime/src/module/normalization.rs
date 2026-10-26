//! Complete executable/source identity inventory and destination-local normalization.
use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    module::VerifiedProgram,
    reload::updates::coverage::CoverageFacts,
};
use kagari_bytecode::{
    module::BytecodeModule,
    program::identity,
    source_package::{SourcePackageError, VerifiedSourcePackage},
};
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

    /// Import exact identities without reissuing product or preparation identity.
    /// The supplied token also governs same-scope and source-only paths. Failed
    /// imports may intern names, but return no partially normalized program.
    pub(crate) fn normalized(
        &self,
        context: &DefinitionContext,
        cancel: &CancellationToken,
    ) -> Result<Self, RuntimeError> {
        cancel
            .check()
            .map_err(|_| identity_error(DefinitionMappingError::Cancelled))?;
        if self.definitions.id() == context.snapshot().id() {
            return Ok(self.clone());
        }
        let (
            modules,
            exports,
            export_targets,
            definition_references,
            update_evidence,
            update_coverage,
            native_applications,
        ) = if self.definition_references.is_empty() {
            (
                self.modules.clone(),
                self.exports.clone(),
                self.export_targets.clone(),
                self.definition_references.clone(),
                self.update_evidence.clone(),
                self.update_coverage.clone(),
                self.native_applications.clone(),
            )
        } else {
            let remap = context
                .import(
                    &self.definitions,
                    self.definition_references.iter().copied(),
                )
                .map_err(|error| identity_error(error.into()))?;
            let target = context.snapshot();
            let mut convert =
                |id: &DefinitionId| remap.map(*id).map_err(DefinitionMappingError::from);
            let mut mapper = DefinitionMapper::new(&mut convert, &target, cancel);
            let modules = self
                .modules
                .iter()
                .map(|module| module.remap_definitions(&mut mapper).map(Arc::new))
                .collect::<Result<_, _>>()
                .map_err(identity_error)?;
            let exports = self
                .exports
                .remap_definitions(&mut mapper)
                .map_err(identity_error)?;
            let export_targets = self
                .export_targets
                .iter()
                .map(|(id, target)| remap.map(*id).map(|id| (id, *target)))
                .collect::<Result<_, _>>()
                .map_err(|error| identity_error(error.into()))?;
            let definition_references = self
                .definition_references
                .iter()
                .map(|id| remap.map(*id))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| identity_error(error.into()))?;
            let update_evidence = self
                .update_evidence
                .remap_definitions(&mut mapper)
                .map_err(identity_error)?;
            let native_applications = self
                .native_applications
                .iter()
                .map(|witness| witness.remap_definitions(&mut mapper))
                .collect::<Result<Vec<_>, _>>()
                .map_err(identity_error)?;
            let update_coverage = self
                .update_coverage
                .remap_definitions(&mut mapper)
                .map_err(identity_error)?;
            (
                modules,
                Arc::new(exports),
                Arc::new(export_targets),
                definition_references.into(),
                Arc::new(update_evidence),
                Arc::new(update_coverage),
                native_applications.into(),
            )
        };
        let definitions = identity::collect(
            &modules.iter().map(Arc::as_ref).collect::<Vec<_>>(),
            context,
            cancel,
        )
        .map_err(|error| {
            if cancel.check().is_err() {
                identity_error(DefinitionMappingError::Cancelled)
            } else {
                RuntimeError::module_validation(error.to_string())
            }
        })?;
        let source_package = self
            .source_package
            .as_ref()
            .map(|package| {
                package
                    .for_product(self.product_identity, &definitions, cancel)
                    .map_err(|error| match error {
                        SourcePackageError::Identity(error) => identity_error(error),
                        error => RuntimeError::module_validation(error.to_string()),
                    })
            })
            .transpose()?;
        cancel
            .check()
            .map_err(|_| identity_error(DefinitionMappingError::Cancelled))?;
        Ok(Self {
            product_identity: self.product_identity,
            source_package,
            update_evidence,
            update_coverage,
            native_applications,
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

fn identity_error(error: DefinitionMappingError) -> RuntimeError {
    match error {
        DefinitionMappingError::Cancelled => RuntimeError::new(
            RuntimeErrorKind::Cancelled,
            "program identity import cancelled",
        ),
        error => RuntimeError::module_validation(error.to_string()),
    }
}
