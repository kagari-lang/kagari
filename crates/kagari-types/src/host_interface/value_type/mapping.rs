//! Contextual identity traversal of the owning metadata records.
use crate::host_interface::value_type::HostValueType;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{
            DefinitionMapper, DefinitionMappingError, DefinitionRecord, check_cancel, map_sequence,
        },
        reference::DefinitionReference,
    },
};

impl<I: DefinitionReference> DefinitionRecord<I> for HostValueType<I> {
    type Rebind<J: DefinitionReference> = HostValueType<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        self.validate()
            .map_err(|_| DefinitionMappingError::InvalidContract)?;
        let mapped = match self {
            Self::Unit => HostValueType::Unit,
            Self::Bool => HostValueType::Bool,
            Self::I32 => HostValueType::I32,
            Self::I64 => HostValueType::I64,
            Self::F32 => HostValueType::F32,
            Self::F64 => HostValueType::F64,
            Self::String => HostValueType::String,
            Self::Opaque(field0) => HostValueType::Opaque(mapper.reference(field0)?),
            Self::Tuple(field0) => HostValueType::Tuple(map_sequence(field0, |value| {
                (value).map_identities(mapper)
            })?),
            Self::Array(field0, field1) => HostValueType::Array(
                Box::new(((field0).as_ref()).map_identities(mapper)?),
                *(field1),
            ),
            Self::Map { key, value, access } => HostValueType::Map {
                key: Box::new(((key).as_ref()).map_identities(mapper)?),
                value: Box::new(((value).as_ref()).map_identities(mapper)?),
                access: *(access),
            },
            Self::Set(field0, field1) => HostValueType::Set(
                Box::new(((field0).as_ref()).map_identities(mapper)?),
                *(field1),
            ),
            Self::Option(field0) => {
                HostValueType::Option(Box::new(((field0).as_ref()).map_identities(mapper)?))
            }
            Self::Result { ok, error } => HostValueType::Result {
                ok: Box::new(((ok).as_ref()).map_identities(mapper)?),
                error: Box::new(((error).as_ref()).map_identities(mapper)?),
            },
        };
        mapped
            .validate()
            .map_err(|_| DefinitionMappingError::InvalidContract)?;
        Ok(mapped)
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.validate()
            .map_err(|_| DefinitionMappingError::InvalidContract)?;
        match self {
            Self::Unit => {}
            Self::Bool => {}
            Self::I32 => {}
            Self::I64 => {}
            Self::F32 => {}
            Self::F64 => {}
            Self::String => {}
            Self::Opaque(field0) => {
                check_cancel(cancel)?;
                visit(field0)?;
            }
            Self::Tuple(field0) => {
                for value0 in field0 {
                    (value0).visit_definitions(visit, cancel)?;
                }
            }
            Self::Array(field0, _) => {
                ((field0).as_ref()).visit_definitions(visit, cancel)?;
            }
            Self::Map {
                key,
                value,
                access: _,
            } => {
                ((key).as_ref()).visit_definitions(visit, cancel)?;
                ((value).as_ref()).visit_definitions(visit, cancel)?;
            }
            Self::Set(field0, _) => {
                ((field0).as_ref()).visit_definitions(visit, cancel)?;
            }
            Self::Option(field0) => {
                ((field0).as_ref()).visit_definitions(visit, cancel)?;
            }
            Self::Result { ok, error } => {
                ((ok).as_ref()).visit_definitions(visit, cancel)?;
                ((error).as_ref()).visit_definitions(visit, cancel)?;
            }
        }
        Ok(())
    }
}
