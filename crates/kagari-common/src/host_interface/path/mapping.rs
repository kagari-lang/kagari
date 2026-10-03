//! Contextual identity traversal of the owning metadata records.
use crate::host_interface::path::{
    HostIndexSegmentDeclaration, HostPathContract, HostPathDeclaration, HostPathInput,
    HostPathSegmentContract, HostPathSegmentDeclaration, HostVirtualSegmentDeclaration,
};
use crate::{
    cancellation::CancellationToken,
    identity::{
        mapping::{
            DefinitionMapper, DefinitionMappingError, DefinitionRecord, check_cancel, map_sequence,
        },
        reference::DefinitionReference,
    },
};

impl<I: DefinitionReference> DefinitionRecord<I> for HostPathDeclaration<I> {
    type Rebind<J: DefinitionReference> = HostPathDeclaration<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(HostPathDeclaration {
            root: mapper.reference(&self.root)?,
            segments: map_sequence(&self.segments, |value| (value).map_identities(mapper))?,
            access: self.access,
            schema_epoch: self.schema_epoch,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        check_cancel(cancel)?;
        visit(&self.root)?;
        for value0 in &self.segments {
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for HostPathSegmentDeclaration<I> {
    type Rebind<J: DefinitionReference> = HostPathSegmentDeclaration<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(match self {
            Self::Field(field0) => HostPathSegmentDeclaration::Field(mapper.reference(field0)?),
            Self::Index(field0) => {
                HostPathSegmentDeclaration::Index((field0).map_identities(mapper)?)
            }
            Self::Virtual(field0) => {
                HostPathSegmentDeclaration::Virtual((field0).map_identities(mapper)?)
            }
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        match self {
            Self::Field(field0) => {
                check_cancel(cancel)?;
                visit(field0)?;
            }
            Self::Index(field0) => {
                (field0).visit_definitions(visit, cancel)?;
            }
            Self::Virtual(field0) => {
                (field0).visit_definitions(visit, cancel)?;
            }
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for HostIndexSegmentDeclaration<I> {
    type Rebind<J: DefinitionReference> = HostIndexSegmentDeclaration<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(HostIndexSegmentDeclaration {
            slot: self.slot,
            collection: self.collection.map_identities(mapper)?,
            index: self.index.map_identities(mapper)?,
            result: self.result.map_identities(mapper)?,
            access: self.access,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.collection.visit_definitions(visit, cancel)?;
        self.index.visit_definitions(visit, cancel)?;
        self.result.visit_definitions(visit, cancel)?;
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for HostVirtualSegmentDeclaration<I> {
    type Rebind<J: DefinitionReference> = HostVirtualSegmentDeclaration<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(HostVirtualSegmentDeclaration {
            name: self.name.clone(),
            result: self.result.map_identities(mapper)?,
            access: self.access,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.result.visit_definitions(visit, cancel)?;
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for HostPathInput<I> {
    type Rebind<J: DefinitionReference> = HostPathInput<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(match self {
            Self::Field { owner } => HostPathInput::Field {
                owner: (owner).map_identities(mapper)?,
            },
            Self::Index {
                slot,
                collection,
                index,
            } => HostPathInput::Index {
                slot: *(slot),
                collection: (collection).map_identities(mapper)?,
                index: (index).map_identities(mapper)?,
            },
            Self::Virtual { name } => HostPathInput::Virtual {
                name: (name).clone(),
            },
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        match self {
            Self::Field { owner } => {
                (owner).visit_definitions(visit, cancel)?;
            }
            Self::Index {
                slot: _,
                collection,
                index,
            } => {
                (collection).visit_definitions(visit, cancel)?;
                (index).visit_definitions(visit, cancel)?;
            }
            Self::Virtual { name: _ } => {}
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for HostPathSegmentContract<I> {
    type Rebind<J: DefinitionReference> = HostPathSegmentContract<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(HostPathSegmentContract {
            input: self.input.map_identities(mapper)?,
            result: self.result.map_identities(mapper)?,
            access: self.access,
            member_fingerprint: self.member_fingerprint,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.input.visit_definitions(visit, cancel)?;
        self.result.visit_definitions(visit, cancel)?;
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for HostPathContract<I> {
    type Rebind<J: DefinitionReference> = HostPathContract<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(HostPathContract {
            root_fingerprint: self.root_fingerprint,
            result: self.result.map_identities(mapper)?,
            schema_epoch: self.schema_epoch,
            access: self.access,
            segments: map_sequence(&self.segments, |value| (value).map_identities(mapper))?,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.result.visit_definitions(visit, cancel)?;
        for value0 in &self.segments {
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}
