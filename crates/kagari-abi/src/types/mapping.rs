//! Contextual identity traversal of the owning metadata records.
use crate::types::{
    AssociatedConstAbi, AssociatedTypeAbi, AssociatedTypeFamilyAbi, ConcreteFunctionIdentity,
    ConstAbi, ConstraintAbi, FieldAbi, FunctionAbi, GenericBoundAbi, GenericParameterAbi,
    InterfaceTableAbi, ModuleAbi, NativeDeclaration, NominalAbiType, ParameterAbi, PublicAbiItem,
    TraitAbi, TraitContract, TypeAbi, VariantAbi,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{
            DefinitionMapper, DefinitionMappingError, DefinitionRecord, check_cancel, map_entries,
            map_sequence,
        },
        reference::DefinitionReference,
    },
};

impl<I: DefinitionReference> DefinitionRecord<I> for ModuleAbi<I> {
    type Rebind<J: DefinitionReference> = ModuleAbi<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(ModuleAbi {
            native_declarations: map_sequence(&self.native_declarations, |value| {
                (value).map_identities(mapper)
            })?,
            public_items: map_sequence(&self.public_items, |value| (value).map_identities(mapper))?,
            trait_contracts: map_sequence(&self.trait_contracts, |value| {
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
        for value0 in &self.native_declarations {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in &self.public_items {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in &self.trait_contracts {
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for PublicAbiItem<I> {
    type Rebind<J: DefinitionReference> = PublicAbiItem<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(match self {
            Self::Function(field0) => PublicAbiItem::Function((field0).map_identities(mapper)?),
            Self::Const(field0) => PublicAbiItem::Const((field0).map_identities(mapper)?),
            Self::Type(field0) => PublicAbiItem::Type((field0).map_identities(mapper)?),
            Self::Trait(field0) => PublicAbiItem::Trait((field0).map_identities(mapper)?),
            Self::InterfaceTable(field0) => {
                PublicAbiItem::InterfaceTable(Box::new(((field0).as_ref()).map_identities(mapper)?))
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
            Self::Function(field0) => {
                (field0).visit_definitions(visit, cancel)?;
            }
            Self::Const(field0) => {
                (field0).visit_definitions(visit, cancel)?;
            }
            Self::Type(field0) => {
                (field0).visit_definitions(visit, cancel)?;
            }
            Self::Trait(field0) => {
                (field0).visit_definitions(visit, cancel)?;
            }
            Self::InterfaceTable(field0) => {
                ((field0).as_ref()).visit_definitions(visit, cancel)?;
            }
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for FunctionAbi<I> {
    type Rebind<J: DefinitionReference> = FunctionAbi<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(FunctionAbi {
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

impl<I: DefinitionReference> DefinitionRecord<I> for ParameterAbi<I> {
    type Rebind<J: DefinitionReference> = ParameterAbi<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(ParameterAbi {
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

impl<I: DefinitionReference> DefinitionRecord<I> for ConstAbi<I> {
    type Rebind<J: DefinitionReference> = ConstAbi<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(ConstAbi {
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

impl<I: DefinitionReference> DefinitionRecord<I> for TypeAbi<I> {
    type Rebind<J: DefinitionReference> = TypeAbi<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(TypeAbi {
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

impl<I: DefinitionReference> DefinitionRecord<I> for FieldAbi<I> {
    type Rebind<J: DefinitionReference> = FieldAbi<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(FieldAbi {
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

impl<I: DefinitionReference> DefinitionRecord<I> for VariantAbi<I> {
    type Rebind<J: DefinitionReference> = VariantAbi<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(VariantAbi {
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

impl<I: DefinitionReference> DefinitionRecord<I> for NominalAbiType<I> {
    type Rebind<J: DefinitionReference> = NominalAbiType<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(NominalAbiType {
            declaration: mapper.reference(&self.declaration)?,
            arguments: map_sequence(&self.arguments, |value| (value).map_identities(mapper))?,
            associated_types: map_entries(
                self.associated_types.len(),
                self.associated_types.iter().map(|(key, value)| {
                    Ok((mapper.reference(key)?, (value).map_identities(mapper)?))
                }),
            )?,
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
        for value0 in &self.arguments {
            (value0).visit_definitions(visit, cancel)?;
        }
        for (key0, value0) in &self.associated_types {
            check_cancel(cancel)?;
            visit(key0)?;
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for TraitAbi<I> {
    type Rebind<J: DefinitionReference> = TraitAbi<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(TraitAbi {
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

impl<I: DefinitionReference> DefinitionRecord<I> for AssociatedConstAbi<I> {
    type Rebind<J: DefinitionReference> = AssociatedConstAbi<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(AssociatedConstAbi {
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

impl<I: DefinitionReference> DefinitionRecord<I> for AssociatedTypeAbi<I> {
    type Rebind<J: DefinitionReference> = AssociatedTypeAbi<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(AssociatedTypeAbi {
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

impl<I: DefinitionReference> DefinitionRecord<I> for AssociatedTypeFamilyAbi<I> {
    type Rebind<J: DefinitionReference> = AssociatedTypeFamilyAbi<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(AssociatedTypeFamilyAbi {
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

impl<I: DefinitionReference> DefinitionRecord<I> for InterfaceTableAbi<I> {
    type Rebind<J: DefinitionReference> = InterfaceTableAbi<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(InterfaceTableAbi {
            associated_type_families: map_sequence(&self.associated_type_families, |value| {
                (value).map_identities(mapper)
            })?,
            associated_consts: map_sequence(&self.associated_consts, |value| {
                (value).map_identities(mapper)
            })?,
            host_bridge: self.host_bridge,
            declaration: mapper.reference(&self.declaration)?,
            name: self.name.clone(),
            generic_params: map_sequence(&self.generic_params, |value| {
                (value).map_identities(mapper)
            })?,
            bounds: map_sequence(&self.bounds, |value| (value).map_identities(mapper))?,
            trait_type: self.trait_type.map_identities(mapper)?,
            for_type: self.for_type.map_identities(mapper)?,
            methods: map_sequence(&self.methods, |value| (value).map_identities(mapper))?,
        })
    }
    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for value0 in &self.associated_type_families {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in &self.associated_consts {
            (value0).visit_definitions(visit, cancel)?;
        }
        check_cancel(cancel)?;
        visit(&self.declaration)?;
        for value0 in &self.generic_params {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in &self.bounds {
            (value0).visit_definitions(visit, cancel)?;
        }
        self.trait_type.visit_definitions(visit, cancel)?;
        self.for_type.visit_definitions(visit, cancel)?;
        for value0 in &self.methods {
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for TraitContract<I> {
    type Rebind<J: DefinitionReference> = TraitContract<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(TraitContract {
            declaration: mapper.reference(&self.declaration)?,
            abi: self.abi.map_identities(mapper)?,
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
        self.abi.visit_definitions(visit, cancel)?;
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for ConcreteFunctionIdentity<I> {
    type Rebind<J: DefinitionReference> = ConcreteFunctionIdentity<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(ConcreteFunctionIdentity {
            declaration: mapper.reference(&self.declaration)?,
            arguments: map_sequence(&self.arguments, |value| (value).map_identities(mapper))?,
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
        for value0 in &self.arguments {
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for GenericParameterAbi<I> {
    type Rebind<J: DefinitionReference> = GenericParameterAbi<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(GenericParameterAbi {
            owner: mapper.reference(&self.owner)?,
            position: self.position,
        })
    }
    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        check_cancel(cancel)?;
        visit(&self.owner)?;
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for GenericBoundAbi<I> {
    type Rebind<J: DefinitionReference> = GenericBoundAbi<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(GenericBoundAbi {
            ty: self.ty.map_identities(mapper)?,
            constraints: map_sequence(&self.constraints, |value| (value).map_identities(mapper))?,
        })
    }
    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.ty.visit_definitions(visit, cancel)?;
        for value0 in &self.constraints {
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for ConstraintAbi<I> {
    type Rebind<J: DefinitionReference> = ConstraintAbi<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(match self {
            Self::Standard(field0) => ConstraintAbi::Standard(*(field0)),
            Self::Trait(field0) => ConstraintAbi::Trait((field0).map_identities(mapper)?),
        })
    }
    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        match self {
            Self::Standard(_) => {}
            Self::Trait(field0) => {
                (field0).visit_definitions(visit, cancel)?;
            }
        }
        Ok(())
    }
}
