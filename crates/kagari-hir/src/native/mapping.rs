//! Explicit traversal of the owning HIR records.

use crate::native::{NativeBinding, NativeTypeKind};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};

impl RemapDefinitions for NativeBinding {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(match self {
            Self::Entry(field0) => NativeBinding::Entry(mapper.reference(field0)?),
            Self::Host(field0) => NativeBinding::Host(*(field0)),
            Self::Default(field0) => NativeBinding::Default((field0).remap_definitions(mapper)?),
        })
    }
}

impl DefinitionReferences for NativeBinding {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        match self {
            Self::Entry(field0) => {
                check_cancel(cancel)?;
                visit(field0)?;
            }
            Self::Host(_) => {}
            Self::Default(field0) => {
                (field0).visit_definition_ids(visit, cancel)?;
            }
        }
        Ok(())
    }
}

impl RemapDefinitions for NativeTypeKind {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(match self {
            Self::Storage {
                declaration,
                arity,
                layout,
            } => NativeTypeKind::Storage {
                declaration: mapper.reference(declaration)?,
                arity: *(arity),
                layout: *(layout),
            },
            Self::String => NativeTypeKind::String,
            Self::HashMap => NativeTypeKind::HashMap,
            Self::HashSet => NativeTypeKind::HashSet,
            Self::Iter => NativeTypeKind::Iter,
            Self::Range(field0) => NativeTypeKind::Range(*(field0)),
        })
    }
}

impl DefinitionReferences for NativeTypeKind {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        match self {
            Self::Storage {
                declaration,
                arity: _,
                layout: _,
            } => {
                check_cancel(cancel)?;
                visit(declaration)?;
            }
            Self::String => {}
            Self::HashMap => {}
            Self::HashSet => {}
            Self::Iter => {}
            Self::Range(_) => {}
        }
        Ok(())
    }
}
