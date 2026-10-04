//! Scoped identity traversal for semantic declarations.
use crate::declaration::NativeDeclaration;
use crate::declaration::{
    AssociatedConstDef, AssociatedTypeDef, AssociatedTypeFamily, ConstDef, FieldDef, FnDecl, Param,
    TraitDef, TypeDef, VariantDef,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{
            DefinitionMapper, DefinitionMappingError, DefinitionRecord, check_cancel, map_sequence,
        },
        reference::DefinitionReference,
    },
};

impl<I: DefinitionReference> DefinitionRecord<I> for FnDecl<I> {
    type Rebind<J: DefinitionReference> = FnDecl<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(FnDecl {
            method_policy: self.method_policy,
            name: self.name.clone(),
            implementation: self.implementation.map_identities(mapper)?,
            generic_params: map_sequence(&self.generic_params, |value| {
                (value).map_identities(mapper)
            })?,
            bounds: map_sequence(&self.bounds, |value| (value).map_identities(mapper))?,
            params: map_sequence(&self.params, |value| (value).map_identities(mapper))?,
            return_type: self.return_type.map_identities(mapper)?,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.implementation.visit_definitions(visit, cancel)?;
        for value0 in &self.generic_params {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in &self.bounds {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in &self.params {
            (value0).visit_definitions(visit, cancel)?;
        }
        self.return_type.visit_definitions(visit, cancel)?;
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for Param<I> {
    type Rebind<J: DefinitionReference> = Param<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(Param {
            name: self.name.clone(),
            ty: self.ty.map_identities(mapper)?,
            mutable: self.mutable,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.ty.visit_definitions(visit, cancel)?;
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for ConstDef<I> {
    type Rebind<J: DefinitionReference> = ConstDef<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(ConstDef {
            name: self.name.clone(),
            ty: self.ty.map_identities(mapper)?,
            value: self.value.clone(),
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.ty.visit_definitions(visit, cancel)?;
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for TypeDef<I> {
    type Rebind<J: DefinitionReference> = TypeDef<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(TypeDef {
            name: self.name.clone(),
            kind: self.kind,
            generic_params: map_sequence(&self.generic_params, |value| {
                (value).map_identities(mapper)
            })?,
            bounds: map_sequence(&self.bounds, |value| (value).map_identities(mapper))?,
            fields: map_sequence(&self.fields, |value| (value).map_identities(mapper))?,
            variants: map_sequence(&self.variants, |value| (value).map_identities(mapper))?,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for value0 in &self.generic_params {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in &self.bounds {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in &self.fields {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in &self.variants {
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for FieldDef<I> {
    type Rebind<J: DefinitionReference> = FieldDef<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(FieldDef {
            name: self.name.clone(),
            ty: self.ty.map_identities(mapper)?,
            mutable: self.mutable,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.ty.visit_definitions(visit, cancel)?;
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for VariantDef<I> {
    type Rebind<J: DefinitionReference> = VariantDef<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(VariantDef {
            reports_failure: self.reports_failure,
            name: self.name.clone(),
            payload: map_sequence(&self.payload, |value| (value).map_identities(mapper))?,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for value0 in &self.payload {
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for TraitDef<I> {
    type Rebind<J: DefinitionReference> = TraitDef<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(TraitDef {
            conversion_adapter: self
                .conversion_adapter
                .as_ref()
                .map(|adapter| adapter.map_identities(mapper))
                .transpose()?,
            storage_access: self.storage_access,
            associated_consts: map_sequence(&self.associated_consts, |value| {
                (value).map_identities(mapper)
            })?,
            name: self.name.clone(),
            supertraits: map_sequence(&self.supertraits, |value| (value).map_identities(mapper))?,
            generic_params: map_sequence(&self.generic_params, |value| {
                (value).map_identities(mapper)
            })?,
            bounds: map_sequence(&self.bounds, |value| (value).map_identities(mapper))?,
            methods: map_sequence(&self.methods, |value| (value).map_identities(mapper))?,
            associated_types: map_sequence(&self.associated_types, |value| {
                (value).map_identities(mapper)
            })?,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        if let Some(adapter) = &self.conversion_adapter {
            adapter.visit_definitions(visit, cancel)?;
        }
        for value0 in &self.associated_consts {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in &self.supertraits {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in &self.generic_params {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in &self.bounds {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in &self.methods {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in &self.associated_types {
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for AssociatedConstDef<I> {
    type Rebind<J: DefinitionReference> = AssociatedConstDef<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(AssociatedConstDef {
            declaration: mapper.reference(&self.declaration)?,
            ty: self.ty.map_identities(mapper)?,
            default_value: self.default_value.clone(),
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        check_cancel(cancel)?;
        visit(&self.declaration)?;
        self.ty.visit_definitions(visit, cancel)?;
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for AssociatedTypeDef<I> {
    type Rebind<J: DefinitionReference> = AssociatedTypeDef<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(AssociatedTypeDef {
            generic_params: map_sequence(&self.generic_params, |value| {
                (value).map_identities(mapper)
            })?,
            parameter_bounds: map_sequence(&self.parameter_bounds, |value| {
                (value).map_identities(mapper)
            })?,
            declaration: mapper.reference(&self.declaration)?,
            bounds: map_sequence(&self.bounds, |value| (value).map_identities(mapper))?,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for value0 in &self.generic_params {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in &self.parameter_bounds {
            (value0).visit_definitions(visit, cancel)?;
        }
        check_cancel(cancel)?;
        visit(&self.declaration)?;
        for value0 in &self.bounds {
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for AssociatedTypeFamily<I> {
    type Rebind<J: DefinitionReference> = AssociatedTypeFamily<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(AssociatedTypeFamily {
            declaration: mapper.reference(&self.declaration)?,
            generic_params: map_sequence(&self.generic_params, |value| {
                (value).map_identities(mapper)
            })?,
            bounds: map_sequence(&self.bounds, |value| (value).map_identities(mapper))?,
            value: self.value.map_identities(mapper)?,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        check_cancel(cancel)?;
        visit(&self.declaration)?;
        for value0 in &self.generic_params {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in &self.bounds {
            (value0).visit_definitions(visit, cancel)?;
        }
        self.value.visit_definitions(visit, cancel)?;
        Ok(())
    }
}
impl<I: DefinitionReference> DefinitionRecord<I> for NativeDeclaration<I> {
    type Rebind<J: DefinitionReference> = NativeDeclaration<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(NativeDeclaration {
            concrete_result: self
                .concrete_result
                .as_ref()
                .map(|value| (value).map_identities(mapper))
                .transpose()?,
            declaration: mapper.reference(&self.declaration)?,
            function: self.function.map_identities(mapper)?,
            callable_requirements: map_sequence(&self.callable_requirements, |value| {
                (value).map_identities(mapper)
            })?,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        if let Some(value0) = self.concrete_result.as_ref() {
            (value0).visit_definitions(visit, cancel)?;
        }
        check_cancel(cancel)?;
        visit(&self.declaration)?;
        self.function.visit_definitions(visit, cancel)?;
        for value0 in &self.callable_requirements {
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}
