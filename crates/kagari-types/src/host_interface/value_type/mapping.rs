//! Contextual identity traversal of the owning metadata records.
use crate::host_interface::value_type::HostValueType;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel, map_sequence},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};

impl RemapDefinitions for HostValueType {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_, DefinitionId, DefinitionId>,
    ) -> Result<Self, DefinitionMappingError> {
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
                (value).remap_definitions(mapper)
            })?),
            Self::Array(field0) => {
                HostValueType::Array(Box::new(((field0).as_ref()).remap_definitions(mapper)?))
            }
            Self::Vec(declaration, item) => HostValueType::Vec(
                mapper.reference(declaration)?,
                Box::new(item.remap_definitions(mapper)?),
            ),
            Self::List(declaration, item) => HostValueType::List(
                mapper.reference(declaration)?,
                Box::new(item.remap_definitions(mapper)?),
            ),
            Self::Map { key, value, access } => HostValueType::Map {
                key: Box::new(((key).as_ref()).remap_definitions(mapper)?),
                value: Box::new(((value).as_ref()).remap_definitions(mapper)?),
                access: *(access),
            },
            Self::Set(field0, field1) => HostValueType::Set(
                Box::new(((field0).as_ref()).remap_definitions(mapper)?),
                *(field1),
            ),
            Self::Option(declaration, field0) => HostValueType::Option(
                mapper.reference(declaration)?,
                Box::new(((field0).as_ref()).remap_definitions(mapper)?),
            ),
            Self::Result {
                declaration,
                ok,
                error,
            } => HostValueType::Result {
                declaration: mapper.reference(declaration)?,
                ok: Box::new(((ok).as_ref()).remap_definitions(mapper)?),
                error: Box::new(((error).as_ref()).remap_definitions(mapper)?),
            },
        };
        mapped
            .validate()
            .map_err(|_| DefinitionMappingError::InvalidContract)?;
        Ok(mapped)
    }
}

impl DefinitionReferences for HostValueType {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
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
                    (value0).visit_definition_ids(visit, cancel)?;
                }
            }
            Self::Array(field0) => {
                ((field0).as_ref()).visit_definition_ids(visit, cancel)?;
            }
            Self::Vec(declaration, item) | Self::List(declaration, item) => {
                check_cancel(cancel)?;
                visit(declaration)?;
                item.visit_definition_ids(visit, cancel)?;
            }
            Self::Map {
                key,
                value,
                access: _,
            } => {
                ((key).as_ref()).visit_definition_ids(visit, cancel)?;
                ((value).as_ref()).visit_definition_ids(visit, cancel)?;
            }
            Self::Set(field0, _) => {
                ((field0).as_ref()).visit_definition_ids(visit, cancel)?;
            }
            Self::Option(declaration, field0) => {
                visit(declaration)?;
                ((field0).as_ref()).visit_definition_ids(visit, cancel)?;
            }
            Self::Result {
                declaration,
                ok,
                error,
            } => {
                visit(declaration)?;
                ((ok).as_ref()).visit_definition_ids(visit, cancel)?;
                ((error).as_ref()).visit_definition_ids(visit, cancel)?;
            }
        }
        Ok(())
    }
}
