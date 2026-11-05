//! Definition traversal and reconstruction for retained signature contracts.

use crate::{
    identity_mapping::map_hash_entries,
    semantic::signatures::{BodySignatures, FunctionImplementation, TypedFunction, TypedParameter},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel, map_sequence},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};

impl RemapDefinitions for BodySignatures {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        let functions = map_sequence(&self.functions, |function| {
            function.remap_definitions(mapper)
        })?
        .into_iter()
        .collect();
        let statics = map_hash_entries(
            self.statics.len(),
            self.statics
                .iter()
                .map(|(id, ty)| Ok((*id, ty.remap_definitions(mapper)?))),
        )?;
        Ok(Self::new(functions, statics))
    }
}

impl DefinitionReferences for BodySignatures {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for function in &self.functions {
            function.visit_definition_ids(visit, cancel)?;
        }
        for ty in self.statics.values() {
            ty.visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}

impl RemapDefinitions for TypedFunction {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(TypedFunction {
            implementation: self.implementation.remap_definitions(mapper)?,
            generic_params: map_sequence(&self.generic_params, |value| {
                (value).remap_definitions(mapper)
            })?,
            bounds: map_hash_entries(
                self.bounds.len(),
                self.bounds.iter().map(|(key, value)| {
                    Ok((
                        (key).remap_definitions(mapper)?,
                        map_sequence(value, |value| (value).remap_definitions(mapper))?,
                    ))
                }),
            )?,
            id: self.id,
            name: self.name.clone(),
            params: map_sequence(self.params.as_slice(), |value| {
                (value).remap_definitions(mapper)
            })?
            .into_iter()
            .collect(),
            return_type: self.return_type.remap_definitions(mapper)?,
        })
    }
}

impl DefinitionReferences for TypedFunction {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.implementation.visit_definition_ids(visit, cancel)?;
        for value0 in &self.generic_params {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for (key0, value0) in &self.bounds {
            (key0).visit_definition_ids(visit, cancel)?;
            for value1 in value0 {
                (value1).visit_definition_ids(visit, cancel)?;
            }
        }
        for value0 in &self.params {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        self.return_type.visit_definition_ids(visit, cancel)?;
        Ok(())
    }
}

impl RemapDefinitions for FunctionImplementation {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(match self {
            Self::Script => FunctionImplementation::Script,
            Self::Native(field0) => {
                FunctionImplementation::Native((field0).remap_definitions(mapper)?)
            }
            Self::Required => FunctionImplementation::Required,
        })
    }
}

impl DefinitionReferences for FunctionImplementation {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        match self {
            Self::Script => {}
            Self::Native(field0) => {
                (field0).visit_definition_ids(visit, cancel)?;
            }
            Self::Required => {}
        }
        Ok(())
    }
}

impl RemapDefinitions for TypedParameter {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(TypedParameter {
            id: self.id,
            writeability: self.writeability,
            name: self.name.clone(),
            ty: self.ty.remap_definitions(mapper)?,
        })
    }
}

impl DefinitionReferences for TypedParameter {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.ty.visit_definition_ids(visit, cancel)?;
        Ok(())
    }
}
