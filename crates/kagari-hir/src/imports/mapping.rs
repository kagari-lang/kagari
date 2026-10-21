//! Remapping source addresses preserves their file, revision, arena and local item.
use crate::imports::{SourceDeclRef, SourceUnit};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};

impl RemapDefinitions for SourceUnit {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        Ok(Self {
            module: mapper.module(self.module)?,
            file: self.file,
            revision: self.revision,
            arena: self.arena,
        })
    }
}

impl DefinitionReferences for SourceUnit {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        visit(&self.module.definition())
    }
}

impl RemapDefinitions for SourceDeclRef {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        Ok(Self {
            unit: self.unit.remap_definitions(mapper)?,
            item: self.item,
        })
    }
}

impl DefinitionReferences for SourceDeclRef {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        self.unit.visit_definition_ids(visit, cancel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{hir::ids::FunctionId, imports::SourceItem, tests::common::lower_ok};
    use kagari_common::identity::{
        DefinitionKind, map::DefinitionContext, metadata::DefinitionMetadata,
        table::DefinitionTableError,
    };

    #[test]
    fn source_addresses_remap_only_module_scope_and_reject_invalid_targets() {
        let lowered = lower_ok("fn run() {}");
        let source = DefinitionContext::new().unwrap();
        source
            .intern_module(lowered.source.module_identity())
            .unwrap();
        let address = SourceDeclRef {
            unit: SourceUnit::of(&lowered, &source.snapshot()),
            item: SourceItem::Function(FunctionId::new(0)),
        };
        let cancel = CancellationToken::default();
        let metadata =
            DefinitionMetadata::checked(source.snapshot(), address.clone(), &cancel).unwrap();
        let target = DefinitionContext::new().unwrap();
        assert!(DefinitionMetadata::checked(target.snapshot(), address.clone(), &cancel).is_err());
        let imported = metadata.import_into(&target, &cancel).unwrap();
        let mapped = imported.records();
        assert_ne!(mapped.unit.module, address.unit.module);
        assert_eq!(mapped.unit.file, address.unit.file);
        assert_eq!(mapped.unit.revision, address.unit.revision);
        assert_eq!(mapped.unit.arena, address.unit.arena);
        assert_eq!(mapped.item, address.item);
        assert_eq!(
            imported.import_into(&source, &cancel).unwrap().records(),
            &address
        );

        let function = target
            .intern_child(
                mapped.unit.module.definition(),
                DefinitionKind::Function,
                "run",
                0,
            )
            .unwrap();
        assert_eq!(
            address.remap_definitions(&mut DefinitionMapper::new(
                &mut |_| Ok(function),
                &target.snapshot(),
                &cancel,
            )),
            Err(DefinitionMappingError::Identity(
                DefinitionTableError::InvalidModuleRoot
            )),
        );
        assert_eq!(
            address.remap_definitions(&mut DefinitionMapper::new(
                &mut |_| Ok(address.unit.module.definition()),
                &target.snapshot(),
                &cancel,
            )),
            Err(DefinitionMappingError::Identity(
                DefinitionTableError::ForeignTable
            )),
        );
    }
}
