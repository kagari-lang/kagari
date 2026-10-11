//! Explicit traversal of the owning HIR records.
use crate::native::{NativeBinding, NativeTypeKind};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, DefinitionRecord, check_cancel},
        reference::DefinitionReference,
    },
};

impl<I: DefinitionReference> DefinitionRecord<I> for NativeBinding<I> {
    type Rebind<J: DefinitionReference> = NativeBinding<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(match self {
            Self::Entry(field0) => NativeBinding::Entry(mapper.reference(field0)?),
            Self::Host(field0) => NativeBinding::Host(*(field0)),
            Self::Default(field0) => NativeBinding::Default((field0).map_identities(mapper)?),
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
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
                (field0).visit_definitions(visit, cancel)?;
            }
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for NativeTypeKind<I> {
    type Rebind<J: DefinitionReference> = NativeTypeKind<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
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

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
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
