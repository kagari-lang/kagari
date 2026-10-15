//! Contextual identity traversal of the owning metadata records.
use crate::declaration::path::{
    HostIndexSegmentDeclaration, HostPathContract, HostPathDeclaration, HostPathInput,
    HostPathSegmentContract, HostPathSegmentDeclaration, HostVirtualSegmentDeclaration,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel, map_sequence},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};

impl RemapDefinitions for HostPathDeclaration {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(HostPathDeclaration {
            root: mapper.reference(&self.root)?,
            segments: map_sequence(&self.segments, |value| (value).remap_definitions(mapper))?,
            access: self.access,
            schema_epoch: self.schema_epoch,
        })
    }
}

impl DefinitionReferences for HostPathDeclaration {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        visit(&self.root)?;
        for value0 in &self.segments {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}

impl RemapDefinitions for HostPathSegmentDeclaration {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(match self {
            Self::Field(field0) => HostPathSegmentDeclaration::Field(mapper.reference(field0)?),
            Self::Index(field0) => {
                HostPathSegmentDeclaration::Index((field0).remap_definitions(mapper)?)
            }
            Self::Virtual(field0) => {
                HostPathSegmentDeclaration::Virtual((field0).remap_definitions(mapper)?)
            }
        })
    }
}

impl DefinitionReferences for HostPathSegmentDeclaration {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        match self {
            Self::Field(field0) => {
                check_cancel(cancel)?;
                visit(field0)?;
            }
            Self::Index(field0) => {
                (field0).visit_definition_ids(visit, cancel)?;
            }
            Self::Virtual(field0) => {
                (field0).visit_definition_ids(visit, cancel)?;
            }
        }
        Ok(())
    }
}

impl RemapDefinitions for HostIndexSegmentDeclaration {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(HostIndexSegmentDeclaration {
            slot: self.slot,
            collection: self.collection.remap_definitions(mapper)?,
            index: self.index.remap_definitions(mapper)?,
            result: self.result.remap_definitions(mapper)?,
            access: self.access,
        })
    }
}

impl DefinitionReferences for HostIndexSegmentDeclaration {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.collection.visit_definition_ids(visit, cancel)?;
        self.index.visit_definition_ids(visit, cancel)?;
        self.result.visit_definition_ids(visit, cancel)?;
        Ok(())
    }
}

impl RemapDefinitions for HostVirtualSegmentDeclaration {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(HostVirtualSegmentDeclaration {
            name: self.name.clone(),
            result: self.result.remap_definitions(mapper)?,
            access: self.access,
        })
    }
}

impl DefinitionReferences for HostVirtualSegmentDeclaration {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.result.visit_definition_ids(visit, cancel)?;
        Ok(())
    }
}

impl RemapDefinitions for HostPathInput {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(match self {
            Self::Field { owner } => HostPathInput::Field {
                owner: (owner).remap_definitions(mapper)?,
            },
            Self::Index {
                slot,
                collection,
                index,
            } => HostPathInput::Index {
                slot: *(slot),
                collection: (collection).remap_definitions(mapper)?,
                index: (index).remap_definitions(mapper)?,
            },
            Self::Virtual { name } => HostPathInput::Virtual {
                name: (name).clone(),
            },
        })
    }
}

impl DefinitionReferences for HostPathInput {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        match self {
            Self::Field { owner } => {
                (owner).visit_definition_ids(visit, cancel)?;
            }
            Self::Index {
                slot: _,
                collection,
                index,
            } => {
                (collection).visit_definition_ids(visit, cancel)?;
                (index).visit_definition_ids(visit, cancel)?;
            }
            Self::Virtual { name: _ } => {}
        }
        Ok(())
    }
}

impl RemapDefinitions for HostPathSegmentContract {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(HostPathSegmentContract {
            input: self.input.remap_definitions(mapper)?,
            result: self.result.remap_definitions(mapper)?,
            access: self.access,
            member_fingerprint: self.member_fingerprint,
        })
    }
}

impl DefinitionReferences for HostPathSegmentContract {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.input.visit_definition_ids(visit, cancel)?;
        self.result.visit_definition_ids(visit, cancel)?;
        Ok(())
    }
}

impl RemapDefinitions for HostPathContract {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(HostPathContract {
            root_fingerprint: self.root_fingerprint,
            result: self.result.remap_definitions(mapper)?,
            schema_epoch: self.schema_epoch,
            access: self.access,
            segments: map_sequence(&self.segments, |value| (value).remap_definitions(mapper))?,
        })
    }
}

impl DefinitionReferences for HostPathContract {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.result.visit_definition_ids(visit, cancel)?;
        for value0 in &self.segments {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}
