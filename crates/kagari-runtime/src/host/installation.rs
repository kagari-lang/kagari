//! Bind prepared semantic owners to physical slots in the existing root service.
use crate::{
    error::RuntimeError,
    host::{HostRegistry, RegisteredHostType},
    metadata::{TypeId, native::MetadataSnapshot},
};
use kagari_common::identity::table::DefinitionId;
use std::collections::HashMap;

pub(crate) struct PreparedHostTypes {
    types: HashMap<TypeId, RegisteredHostType>,
    declarations: HashMap<DefinitionId, TypeId>,
}

impl PreparedHostTypes {
    pub(crate) fn publish(self, host: &mut HostRegistry) {
        host.registered_types = self.types;
        host.type_declarations = self.declarations;
    }
}

impl HostRegistry {
    pub(crate) fn prepare_native_types(
        &self,
        metadata: &MetadataSnapshot,
    ) -> Result<PreparedHostTypes, RuntimeError> {
        let mut types = self.registered_types.clone();
        let mut declarations = self.type_declarations.clone();
        for (declaration, id, policy, abi_fingerprint) in metadata.external_bindings() {
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
        Ok(PreparedHostTypes {
            types,
            declarations,
        })
    }
}
