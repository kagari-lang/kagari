//! Explicit scope traversal; source offsets and document slots are never definition IDs.
use crate::source::{
    AuthoredDeclaration, ExecutableSource, SourcePackage, manifest::SourceManifest,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel, map_sequence},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};

impl DefinitionReferences for SourceManifest {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        self.validate_limits(cancel)?;
        for declaration in &self.declarations {
            check_cancel(cancel)?;
            visit(&declaration.declaration)?;
        }
        Ok(())
    }
}

impl RemapDefinitions for SourceManifest {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        self.validate_limits(mapper.cancellation())?;
        Ok(Self {
            documents: self.documents.clone(),
            declarations: map_sequence(&self.declarations, |record| {
                Ok(AuthoredDeclaration {
                    declaration: mapper.reference(&record.declaration)?,
                    ..record.clone()
                })
            })?,
        })
    }
}

impl DefinitionReferences for SourcePackage {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        self.validate_limits(cancel)?;
        for declaration in &self.declarations {
            check_cancel(cancel)?;
            visit(&declaration.declaration)?;
        }
        for executable in &self.executables {
            executable.target.visit_definition_ids(visit, cancel)?;
            if let Some(authored) = &executable.authored {
                visit(authored)?;
            }
            if let Some(binding) = &executable.native_binding {
                visit(binding)?;
            }
        }
        Ok(())
    }
}

impl RemapDefinitions for SourcePackage {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        self.validate_limits(mapper.cancellation())?;
        Ok(Self {
            executable_fingerprint: self.executable_fingerprint,
            documents: self.documents.clone(),
            declarations: map_sequence(&self.declarations, |record| {
                Ok(AuthoredDeclaration {
                    declaration: mapper.reference(&record.declaration)?,
                    kind: record.kind,
                    document: record.document,
                    range: record.range,
                    fingerprint: record.fingerprint,
                })
            })?,
            executables: map_sequence(&self.executables, |record| {
                Ok(ExecutableSource {
                    target: record.target.remap_definitions(mapper)?,
                    authored: record
                        .authored
                        .map(|id| mapper.reference(&id))
                        .transpose()?,
                    native_binding: record
                        .native_binding
                        .map(|id| mapper.reference(&id))
                        .transpose()?,
                    origin: record.origin,
                })
            })?,
        })
    }
}
