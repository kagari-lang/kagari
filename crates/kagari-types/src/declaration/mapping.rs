//! Scoped identity traversal for semantic declarations.
use crate::declaration::{
    AssociatedConstDef, AssociatedTypeDef, AssociatedTypeFamily, ConstDef, FieldDef, FnDecl,
    NativeDeclaration, Param, TraitDef, TypeDeclaration, TypeDef, VariantDef,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel, map_sequence},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};

impl RemapDefinitions for FnDecl {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(FnDecl {
            host_policy: self.host_policy.clone(),
            method_policy: self.method_policy,
            name: self.name.clone(),
            implementation: self.implementation.remap_definitions(mapper)?,
            generic_params: map_sequence(&self.generic_params, |value| {
                (value).remap_definitions(mapper)
            })?,
            bounds: map_sequence(&self.bounds, |value| (value).remap_definitions(mapper))?,
            params: map_sequence(&self.params, |value| (value).remap_definitions(mapper))?,
            return_type: self.return_type.remap_definitions(mapper)?,
        })
    }
}

impl DefinitionReferences for FnDecl {
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
        for value0 in &self.bounds {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in &self.params {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        self.return_type.visit_definition_ids(visit, cancel)?;
        Ok(())
    }
}

impl RemapDefinitions for Param {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(Param {
            name: self.name.clone(),
            ty: self.ty.remap_definitions(mapper)?,
            mutable: self.mutable,
        })
    }
}

impl DefinitionReferences for Param {
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

impl RemapDefinitions for ConstDef {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(ConstDef {
            name: self.name.clone(),
            ty: self.ty.remap_definitions(mapper)?,
            value: self.value.clone(),
        })
    }
}

impl DefinitionReferences for ConstDef {
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

impl RemapDefinitions for TypeDef {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(TypeDef {
            name: self.name.clone(),
            kind: self.kind,
            generic_params: map_sequence(&self.generic_params, |value| {
                (value).remap_definitions(mapper)
            })?,
            bounds: map_sequence(&self.bounds, |value| (value).remap_definitions(mapper))?,
            fields: map_sequence(&self.fields, |value| (value).remap_definitions(mapper))?,
            variants: map_sequence(&self.variants, |value| (value).remap_definitions(mapper))?,
        })
    }
}

impl DefinitionReferences for TypeDef {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for value0 in &self.generic_params {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in &self.bounds {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in &self.fields {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in &self.variants {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}

impl RemapDefinitions for FieldDef {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(FieldDef {
            host_policy: self.host_policy,
            visibility: self.visibility,
            name: self.name.clone(),
            ty: self.ty.remap_definitions(mapper)?,
            mutable: self.mutable,
        })
    }
}

impl DefinitionReferences for FieldDef {
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

impl RemapDefinitions for VariantDef {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(VariantDef {
            reports_failure: self.reports_failure,
            name: self.name.clone(),
            payload: map_sequence(&self.payload, |value| (value).remap_definitions(mapper))?,
        })
    }
}

impl DefinitionReferences for VariantDef {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for value0 in &self.payload {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}

impl RemapDefinitions for TraitDef {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(TraitDef {
            conversion_adapter: self
                .conversion_adapter
                .as_ref()
                .map(|adapter| adapter.remap_definitions(mapper))
                .transpose()?,
            associated_consts: map_sequence(&self.associated_consts, |value| {
                (value).remap_definitions(mapper)
            })?,
            name: self.name.clone(),
            supertraits: map_sequence(&self.supertraits, |value| {
                (value).remap_definitions(mapper)
            })?,
            generic_params: map_sequence(&self.generic_params, |value| {
                (value).remap_definitions(mapper)
            })?,
            bounds: map_sequence(&self.bounds, |value| (value).remap_definitions(mapper))?,
            methods: map_sequence(&self.methods, |value| (value).remap_definitions(mapper))?,
            associated_types: map_sequence(&self.associated_types, |value| {
                (value).remap_definitions(mapper)
            })?,
        })
    }
}

impl DefinitionReferences for TraitDef {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        if let Some(adapter) = &self.conversion_adapter {
            adapter.visit_definition_ids(visit, cancel)?;
        }
        for value0 in &self.associated_consts {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in &self.supertraits {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in &self.generic_params {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in &self.bounds {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in &self.methods {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in &self.associated_types {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}

impl RemapDefinitions for AssociatedConstDef {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(AssociatedConstDef {
            declaration: mapper.reference(&self.declaration)?,
            ty: self.ty.remap_definitions(mapper)?,
            default_value: self.default_value.clone(),
        })
    }
}

impl DefinitionReferences for AssociatedConstDef {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        visit(&self.declaration)?;
        self.ty.visit_definition_ids(visit, cancel)?;
        Ok(())
    }
}

impl RemapDefinitions for AssociatedTypeDef {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(AssociatedTypeDef {
            generic_params: map_sequence(&self.generic_params, |value| {
                (value).remap_definitions(mapper)
            })?,
            parameter_bounds: map_sequence(&self.parameter_bounds, |value| {
                (value).remap_definitions(mapper)
            })?,
            declaration: mapper.reference(&self.declaration)?,
            bounds: map_sequence(&self.bounds, |value| (value).remap_definitions(mapper))?,
        })
    }
}

impl DefinitionReferences for AssociatedTypeDef {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for value0 in &self.generic_params {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in &self.parameter_bounds {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        check_cancel(cancel)?;
        visit(&self.declaration)?;
        for value0 in &self.bounds {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}

impl RemapDefinitions for AssociatedTypeFamily {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(AssociatedTypeFamily {
            declaration: mapper.reference(&self.declaration)?,
            generic_params: map_sequence(&self.generic_params, |value| {
                (value).remap_definitions(mapper)
            })?,
            bounds: map_sequence(&self.bounds, |value| (value).remap_definitions(mapper))?,
            value: self.value.remap_definitions(mapper)?,
        })
    }
}

impl DefinitionReferences for AssociatedTypeFamily {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        visit(&self.declaration)?;
        for value0 in &self.generic_params {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in &self.bounds {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        self.value.visit_definition_ids(visit, cancel)?;
        Ok(())
    }
}

impl RemapDefinitions for NativeDeclaration {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(NativeDeclaration {
            concrete_result: self
                .concrete_result
                .as_ref()
                .map(|value| (value).remap_definitions(mapper))
                .transpose()?,
            declaration: mapper.reference(&self.declaration)?,
            function: self.function.remap_definitions(mapper)?,
            callable_requirements: map_sequence(&self.callable_requirements, |value| {
                (value).remap_definitions(mapper)
            })?,
        })
    }
}

impl DefinitionReferences for NativeDeclaration {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        if let Some(value0) = self.concrete_result.as_ref() {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        check_cancel(cancel)?;
        visit(&self.declaration)?;
        self.function.visit_definition_ids(visit, cancel)?;
        for value0 in &self.callable_requirements {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}

impl RemapDefinitions for TypeDeclaration {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(Self {
            declaration: mapper.reference(&self.declaration)?,
            definition: self.definition.remap_definitions(mapper)?,
        })
    }
}

impl DefinitionReferences for TypeDeclaration {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        visit(&self.declaration)?;
        self.definition.visit_definition_ids(visit, cancel)
    }
}
