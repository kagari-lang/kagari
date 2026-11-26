//! Project execution-only registration facts into the already checked module's identity scope.
use super::AdmissionError;
use crate::collections::HashMap;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError},
        metadata::DefinitionMetadata,
        scoped::RemapDefinitions,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{
        TypeDeclaration, TypeDefKind,
        module::ModuleDecl,
        path::{HostPathDeclaration, NativePathOperation},
    },
    reference::ReferenceAccess,
};

#[derive(Default)]
pub(super) struct ExecutionRegistration {
    pub(super) host_schemas: Vec<TypeDeclaration>,
    pub(super) host_paths: Vec<HostPathDeclaration>,
    pub(super) receiver_loans: HashMap<DefinitionId, ReferenceAccess>,
    pub(super) path_operations: HashMap<DefinitionId, NativePathOperation>,
}

impl ExecutionRegistration {
    pub(super) fn project(
        provider: Option<&DefinitionMetadata<ModuleDecl>>,
        target: &DefinitionTable,
        cancel: &CancellationToken,
    ) -> Result<Self, AdmissionError> {
        let mut result = Self::default();
        let Some(provider) = provider else {
            return Ok(result);
        };
        let source = provider.definitions();
        let mut resolve = |id: &DefinitionId| {
            target
                .lookup(&source.resolve(*id)?.to_path())
                .ok_or(DefinitionMappingError::InvalidContract)
        };
        let mut mapper = DefinitionMapper::new(&mut resolve, target, cancel);
        let invalid = |error| match error {
            DefinitionMappingError::Cancelled => AdmissionError::Cancelled,
            _ => AdmissionError::InvalidModule("registration identity missing from checked module"),
        };
        let records = provider.records();
        for ty in &records.types {
            cancel.check().map_err(|_| AdmissionError::Cancelled)?;
            if matches!(ty.kind, TypeDefKind::Host(_)) {
                let owner = records
                    .definition(ty.kind.definition_kind(), &ty.name, source)
                    .map_err(|_| AdmissionError::InvalidModule("registered Host identity"))?;
                result.host_schemas.push(TypeDeclaration {
                    declaration: mapper.reference(&owner).map_err(invalid)?,
                    definition: ty.remap_definitions(&mut mapper).map_err(invalid)?,
                });
            }
        }
        result.host_paths = records
            .paths
            .remap_definitions(&mut mapper)
            .map_err(invalid)?;
        for (id, operation) in &records.path_operations {
            let id = mapper.reference(id).map_err(invalid)?;
            result.path_operations.insert(
                id,
                operation.remap_definitions(&mut mapper).map_err(invalid)?,
            );
        }
        for (index, implementation) in records.implementations.iter().enumerate() {
            for method in &implementation.methods {
                cancel.check().map_err(|_| AdmissionError::Cancelled)?;
                if let Some(access) = method.method_policy.receiver_access
                    && matches!(method.implementation, CallableImplementation::Native(_))
                {
                    let owner = records.implementation_id(index, source).map_err(|_| {
                        AdmissionError::InvalidModule("registered implementation identity")
                    })?;
                    let member = ModuleDecl::method_id(owner, &method.name, source)
                        .map_err(|_| AdmissionError::InvalidModule("registered method identity"))?;
                    result
                        .receiver_loans
                        .insert(mapper.reference(&member).map_err(invalid)?, access);
                }
            }
        }
        Ok(result)
    }
}
