//! Bind prepared semantic owners to physical slots in the existing root service.
use crate::{
    error::RuntimeError,
    host::{
        HostPathAdapter, HostPathDescriptor, HostPathDescriptorId, HostRegistry, HostRegistryId,
        RegisteredHostType,
    },
    metadata::TypeId,
    native::registry::NativeRegistry,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{map::DefinitionContext, metadata::DefinitionMetadata, table::DefinitionId},
};
use kagari_types::declaration::host::PathAccess;
use std::collections::HashMap;

/// Checked registration facts without host roots, dirty state or registry identity.
#[derive(Debug, Clone, Default)]
pub(crate) struct PreparedHostTypes {
    types: HashMap<TypeId, RegisteredHostType>,
    declarations: HashMap<DefinitionId, TypeId>,
    next_path_descriptor_id: usize,
    path_descriptors: HashMap<HostPathDescriptorId, HostPathDescriptor>,
    path_adapters: HashMap<HostPathDescriptorId, HostPathAdapter>,
}

impl HostPathAdapter {
    pub(crate) fn check_access(&self, access: PathAccess) -> Result<(), RuntimeError> {
        if access == PathAccess::None
            || self.read.is_none()
            || (access == PathAccess::ReadWrite && self.prepare_write.is_none())
            || (access == PathAccess::ReadOnly && self.prepare_write.is_some())
        {
            return Err(RuntimeError::metadata_conflict(
                "manual path binding differs from declared read/write access",
            ));
        }
        Ok(())
    }
}

impl PreparedHostTypes {
    pub(crate) fn publish(self, host: &mut HostRegistry) {
        host.registered_types = self.types;
        host.type_declarations = self.declarations;
        host.next_path_descriptor_id = self.next_path_descriptor_id;
        host.path_descriptors = self.path_descriptors;
        host.path_adapters = self.path_adapters;
    }
}

impl HostRegistry {
    /// Construct runtime-local identity and roots around checked installation facts.
    /// An empty registry has no facts to validate; installed maps are copied directly
    /// without first initializing and replacing four empty randomized maps.
    pub(crate) fn new(
        definitions: DefinitionContext,
        prepared: Option<&PreparedHostTypes>,
    ) -> Self {
        let prepared = prepared.cloned().unwrap_or_default();
        Self {
            definitions,
            owner: HostRegistryId::default(),
            next_path_descriptor_id: prepared.next_path_descriptor_id,
            type_declarations: prepared.declarations,
            registered_types: prepared.types,
            roots: Default::default(),
            path_descriptors: prepared.path_descriptors,
            path_adapters: prepared.path_adapters,
            dirty_paths: Default::default(),
        }
    }

    pub(crate) fn prepare_native_registration(
        &self,
        registry: &NativeRegistry,
    ) -> Result<PreparedHostTypes, RuntimeError> {
        let mut types = self.registered_types.clone();
        let mut declarations = self.type_declarations.clone();
        for (declaration, id, policy, abi_fingerprint) in registry.metadata.external_bindings() {
            let record = RegisteredHostType {
                policy,
                abi_fingerprint,
            };
            if declarations
                .insert(declaration, id)
                .is_some_and(|previous| previous != id)
                || types
                    .insert(id, record)
                    .is_some_and(|previous| previous != record)
            {
                return Err(RuntimeError::metadata_conflict(
                    "external type slot differs from its registered owner",
                ));
            }
        }
        let mut staged = Self::new(self.definitions.clone(), None);
        staged.registered_types = types;
        staged.type_declarations = declarations;
        staged.path_descriptors = self.path_descriptors.clone();
        staged.path_adapters = self.path_adapters.clone();
        staged.next_path_descriptor_id = self.next_path_descriptor_id;
        let metadata = registry.metadata.runtime_registry();
        let definitions = registry.catalog.definitions();
        for (root, paths) in registry.catalog.paths.entries() {
            let adapters = registry.path_bindings.get_id(root).ok_or_else(|| {
                RuntimeError::metadata_conflict("manual path owner has no installed bindings")
            })?;
            if paths.len() != adapters.len() {
                return Err(RuntimeError::metadata_conflict(
                    "manual path binding count differs",
                ));
            }
            for (path, adapter) in paths.iter().zip(adapters) {
                adapter.check_access(path.access)?;
                if staged
                    .path_descriptors
                    .values()
                    .any(|descriptor| descriptor.declaration == *path)
                {
                    continue;
                }
                let declaration = DefinitionMetadata::checked(
                    definitions.clone(),
                    path.clone(),
                    &CancellationToken::default(),
                )
                .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;
                let id = staged.register_path(&declaration, &metadata)?;
                staged.register_path_adapter(id, adapter.clone())?;
            }
        }
        Ok(PreparedHostTypes {
            types: staged.registered_types,
            declarations: staged.type_declarations,
            next_path_descriptor_id: staged.next_path_descriptor_id,
            path_descriptors: staged.path_descriptors,
            path_adapters: staged.path_adapters,
        })
    }
}
