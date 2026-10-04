//! Explicit traversal of the owning HIR records.
use crate::identity_mapping::map_hash_entries;
use crate::typeck::table::{
    CallTarget, ConstraintTarget, ResolvedAssociatedConst, ResolvedCall, ResolvedEnumConstructor,
    ResolvedHostPath, ResolvedHostPlacePath, ResolvedInterfaceCoercion,
    ResolvedInterfaceImplementation, ResolvedIteration, ResolvedStructInit, ResolvedTypeRef,
    TraitImplementation, TypeTable, TypeTarget,
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

impl<I: DefinitionReference> DefinitionRecord<I> for ConstraintTarget<I> {
    type Rebind<J: DefinitionReference> = ConstraintTarget<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(match self {
            Self::Standard(field0) => ConstraintTarget::Standard(*(field0)),
            Self::Trait(field0) => ConstraintTarget::Trait((field0).map_identities(mapper)?),
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

impl<I: DefinitionReference> DefinitionRecord<I> for TypeTarget<I> {
    type Rebind<J: DefinitionReference> = TypeTarget<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(match self {
            Self::OpaqueType(field0) => TypeTarget::OpaqueType(*(field0)),
            Self::Host(field0) => TypeTarget::Host(*(field0)),
            Self::Source(field0) => TypeTarget::Source(mapper.reference(field0)?),
            Self::AssociatedType(field0) => TypeTarget::AssociatedType(mapper.reference(field0)?),
            Self::Struct(field0) => TypeTarget::Struct(*(field0)),
            Self::Enum(field0) => TypeTarget::Enum(*(field0)),
            Self::Trait(field0) => TypeTarget::Trait(*(field0)),
            Self::Generic(field0) => TypeTarget::Generic(*(field0)),
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        match self {
            Self::OpaqueType(_) => {}
            Self::Host(_) => {}
            Self::Source(field0) => {
                check_cancel(cancel)?;
                visit(field0)?;
            }
            Self::AssociatedType(field0) => {
                check_cancel(cancel)?;
                visit(field0)?;
            }
            Self::Struct(_) => {}
            Self::Enum(_) => {}
            Self::Trait(_) => {}
            Self::Generic(_) => {}
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for ResolvedTypeRef<I> {
    type Rebind<J: DefinitionReference> = ResolvedTypeRef<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(ResolvedTypeRef {
            ty: self.ty.map_identities(mapper)?,
            target: self
                .target
                .as_ref()
                .map(|value| (value).map_identities(mapper))
                .transpose()?,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.ty.visit_definitions(visit, cancel)?;
        if let Some(value0) = self.target.as_ref() {
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for CallTarget<I> {
    type Rebind<J: DefinitionReference> = CallTarget<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(match self {
            Self::TerminatingCallee => CallTarget::TerminatingCallee,
            Self::SourceFunction(field0) => CallTarget::SourceFunction(mapper.reference(field0)?),
            Self::HostFunction(field0) => CallTarget::HostFunction(*(field0)),
            Self::Function(field0) => CallTarget::Function(*(field0)),
            Self::Value => CallTarget::Value,
            Self::RuntimeHelper(field0) => CallTarget::RuntimeHelper(*(field0)),
            Self::TraitMethod { method, interface } => CallTarget::TraitMethod {
                method: mapper.reference(method)?,
                interface: (interface).map_identities(mapper)?,
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
            Self::TerminatingCallee => {}
            Self::SourceFunction(field0) => {
                check_cancel(cancel)?;
                visit(field0)?;
            }
            Self::HostFunction(_) => {}
            Self::Function(_) => {}
            Self::Value => {}
            Self::RuntimeHelper(_) => {}
            Self::TraitMethod { method, interface } => {
                check_cancel(cancel)?;
                visit(method)?;
                (interface).visit_definitions(visit, cancel)?;
            }
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for ResolvedCall<I> {
    type Rebind<J: DefinitionReference> = ResolvedCall<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(ResolvedCall {
            target: self.target.map_identities(mapper)?,
            receiver: self.receiver,
            type_arguments: map_sequence(&self.type_arguments, |value| {
                (value).map_identities(mapper)
            })?,
            signature: self
                .signature
                .as_ref()
                .map(|value| (value).map_identities(mapper))
                .transpose()?,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.target.visit_definitions(visit, cancel)?;
        for value0 in &self.type_arguments {
            (value0).visit_definitions(visit, cancel)?;
        }
        if let Some(value0) = self.signature.as_ref() {
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for ResolvedStructInit<I> {
    type Rebind<J: DefinitionReference> = ResolvedStructInit<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(ResolvedStructInit {
            structure: mapper.reference(&self.structure)?,
            fields: map_sequence(&self.fields, |value| {
                (value)
                    .as_ref()
                    .map(|value| mapper.reference(value))
                    .transpose()
            })?,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        check_cancel(cancel)?;
        visit(&self.structure)?;
        for value0 in &self.fields {
            if let Some(value1) = (value0).as_ref() {
                check_cancel(cancel)?;
                visit(value1)?;
            }
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for ResolvedEnumConstructor<I> {
    type Rebind<J: DefinitionReference> = ResolvedEnumConstructor<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(ResolvedEnumConstructor {
            enumeration: mapper.reference(&self.enumeration)?,
            variant: self
                .variant
                .as_ref()
                .map(|value| mapper.reference(value))
                .transpose()?,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        check_cancel(cancel)?;
        visit(&self.enumeration)?;
        if let Some(value0) = self.variant.as_ref() {
            check_cancel(cancel)?;
            visit(value0)?;
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for TraitImplementation<I> {
    type Rebind<J: DefinitionReference> = TraitImplementation<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(TraitImplementation {
            declaration: mapper.reference(&self.declaration)?,
            parameters: map_sequence(&self.parameters, |value| (value).map_identities(mapper))?,
            bounds: map_hash_entries(
                self.bounds.len(),
                self.bounds.iter().map(|(key, value)| {
                    Ok((
                        (key).map_identities(mapper)?,
                        map_sequence(value, |value| (value).map_identities(mapper))?,
                    ))
                }),
            )?,
            methods: map_hash_entries(
                self.methods.len(),
                self.methods
                    .iter()
                    .map(|(key, value)| Ok((mapper.reference(key)?, *(value)))),
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
        for value0 in &self.parameters {
            (value0).visit_definitions(visit, cancel)?;
        }
        for (key0, value0) in &self.bounds {
            (key0).visit_definitions(visit, cancel)?;
            for value1 in value0 {
                (value1).visit_definitions(visit, cancel)?;
            }
        }
        for key0 in self.methods.keys() {
            check_cancel(cancel)?;
            visit(key0)?;
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for ResolvedIteration<I> {
    type Rebind<J: DefinitionReference> = ResolvedIteration<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(ResolvedIteration {
            into_interface: self.into_interface.map_identities(mapper)?,
            iterator: self.iterator.map_identities(mapper)?,
            next_interface: self.next_interface.map_identities(mapper)?,
            item: self.item.map_identities(mapper)?,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.into_interface.visit_definitions(visit, cancel)?;
        self.iterator.visit_definitions(visit, cancel)?;
        self.next_interface.visit_definitions(visit, cancel)?;
        self.item.visit_definitions(visit, cancel)?;
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for TypeTable<I> {
    type Rebind<J: DefinitionReference> = TypeTable<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(TypeTable {
            inference_holes: map_hash_entries(
                self.inference_holes.len(),
                self.inference_holes
                    .iter()
                    .map(|(key, value)| Ok((*(key), (value).map_identities(mapper)?))),
            )?,
            iterations: map_hash_entries(
                self.iterations.len(),
                self.iterations
                    .iter()
                    .map(|(key, value)| Ok((*(key), (value).map_identities(mapper)?))),
            )?,
            propagations: map_hash_entries(
                self.propagations.len(),
                self.propagations
                    .iter()
                    .map(|(key, value)| Ok((*key, value.map_identities(mapper)?))),
            )?,
            protocol_receivers: map_hash_entries(
                self.protocol_receivers.len(),
                self.protocol_receivers
                    .iter()
                    .map(|(key, value)| Ok((*(key), (value).map_identities(mapper)?))),
            )?,
            associated_consts: map_hash_entries(
                self.associated_consts.len(),
                self.associated_consts
                    .iter()
                    .map(|(key, value)| Ok((*(key), (value).map_identities(mapper)?))),
            )?,
            resolving_types: self.resolving_types.clone(),
            associated_bounds: map_hash_entries(
                self.associated_bounds.len(),
                self.associated_bounds.iter().map(|(key, value)| {
                    Ok((
                        mapper.reference(key)?,
                        map_sequence(value, |value| (value).map_identities(mapper))?,
                    ))
                }),
            )?,
            associated_type_parameters: map_hash_entries(
                self.associated_type_parameters.len(),
                self.associated_type_parameters.iter().map(|(key, value)| {
                    Ok((mapper.reference(key)?, (value).map_identities(mapper)?))
                }),
            )?,
            associated_type_families: map_hash_entries(
                self.associated_type_families.len(),
                self.associated_type_families.iter().map(|(key, value)| {
                    Ok((mapper.reference(key)?, (value).map_identities(mapper)?))
                }),
            )?,
            host_place_paths: map_hash_entries(
                self.host_place_paths.len(),
                self.host_place_paths
                    .iter()
                    .map(|(key, value)| Ok((*(key), (value).map_identities(mapper)?))),
            )?,
            host_paths: map_hash_entries(
                self.host_paths.len(),
                self.host_paths
                    .iter()
                    .map(|(key, value)| Ok((*(key), (value).map_identities(mapper)?))),
            )?,
            implementations: map_hash_entries(
                self.implementations.len(),
                self.implementations.iter().map(|(key, value)| {
                    Ok((
                        (
                            (key).0.map_identities(mapper)?,
                            (key).1.map_identities(mapper)?,
                        ),
                        (value).map_identities(mapper)?,
                    ))
                }),
            )?,
            constraints: map_hash_entries(
                self.constraints.len(),
                self.constraints.iter().map(|(key, value)| {
                    Ok((
                        *(key),
                        (value)
                            .as_ref()
                            .map(|value| (value).map_identities(mapper))
                            .transpose()?,
                    ))
                }),
            )?,
            type_refs: map_hash_entries(
                self.type_refs.len(),
                self.type_refs
                    .iter()
                    .map(|(key, value)| Ok((*(key), (value).map_identities(mapper)?))),
            )?,
            field_types: map_hash_entries(
                self.field_types.len(),
                self.field_types
                    .iter()
                    .map(|(key, value)| Ok((*(key), (value).map_identities(mapper)?))),
            )?,
            expr_fields: map_hash_entries(
                self.expr_fields.len(),
                self.expr_fields
                    .iter()
                    .map(|(key, value)| Ok((*(key), mapper.reference(value)?))),
            )?,
            place_fields: map_hash_entries(
                self.place_fields.len(),
                self.place_fields
                    .iter()
                    .map(|(key, value)| Ok((*(key), mapper.reference(value)?))),
            )?,
            place_indexes: map_hash_entries(
                self.place_indexes.len(),
                self.place_indexes
                    .iter()
                    .map(|(key, value)| Ok((*(key), (value).map_identities(mapper)?))),
            )?,
            struct_inits: map_hash_entries(
                self.struct_inits.len(),
                self.struct_inits
                    .iter()
                    .map(|(key, value)| Ok((*(key), (value).map_identities(mapper)?))),
            )?,
            enum_constructors: map_hash_entries(
                self.enum_constructors.len(),
                self.enum_constructors
                    .iter()
                    .map(|(key, value)| Ok((*(key), (value).map_identities(mapper)?))),
            )?,
            exprs: map_hash_entries(
                self.exprs.len(),
                self.exprs
                    .iter()
                    .map(|(key, value)| Ok((*(key), (value).map_identities(mapper)?))),
            )?,
            callable_coercions: map_hash_entries(
                self.callable_coercions.len(),
                self.callable_coercions.iter().map(|(key, value)| {
                    Ok((
                        *(key),
                        (
                            (value).0.map_identities(mapper)?,
                            (value).1.map_identities(mapper)?,
                        ),
                    ))
                }),
            )?,
            interface_coercions: map_hash_entries(
                self.interface_coercions.len(),
                self.interface_coercions
                    .iter()
                    .map(|(key, value)| Ok((*(key), (value).map_identities(mapper)?))),
            )?,
            locals: map_hash_entries(
                self.locals.len(),
                self.locals
                    .iter()
                    .map(|(key, value)| Ok((*(key), (value).map_identities(mapper)?))),
            )?,
            places: map_hash_entries(
                self.places.len(),
                self.places
                    .iter()
                    .map(|(key, value)| Ok((*(key), (value).map_identities(mapper)?))),
            )?,
            calls: map_hash_entries(
                self.calls.len(),
                self.calls
                    .iter()
                    .map(|(key, value)| Ok((*(key), (value).map_identities(mapper)?))),
            )?,
            scalars: self.scalars.clone(),
            pattern_scalars: self.pattern_scalars.clone(),
            pattern_ranges: self.pattern_ranges.clone(),
            pattern_fields: map_hash_entries(
                self.pattern_fields.len(),
                self.pattern_fields.iter().map(|(key, value)| {
                    Ok((
                        *(key),
                        map_sequence(value, |value| mapper.reference(value))?,
                    ))
                }),
            )?,
            pattern_variants: map_hash_entries(
                self.pattern_variants.len(),
                self.pattern_variants
                    .iter()
                    .map(|(key, value)| Ok((*(key), mapper.reference(value)?))),
            )?,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for value0 in self.inference_holes.values() {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value in self.propagations.values() {
            value.visit_definitions(visit, cancel)?;
        }
        for value0 in self.iterations.values() {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in self.protocol_receivers.values() {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in self.associated_consts.values() {
            (value0).visit_definitions(visit, cancel)?;
        }
        for (key0, value0) in &self.associated_bounds {
            check_cancel(cancel)?;
            visit(key0)?;
            for value1 in value0 {
                (value1).visit_definitions(visit, cancel)?;
            }
        }
        for (key0, value0) in &self.associated_type_parameters {
            check_cancel(cancel)?;
            visit(key0)?;
            (value0).visit_definitions(visit, cancel)?;
        }
        for (key0, value0) in &self.associated_type_families {
            check_cancel(cancel)?;
            visit(key0)?;
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in self.host_place_paths.values() {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in self.host_paths.values() {
            (value0).visit_definitions(visit, cancel)?;
        }
        for (key0, value0) in &self.implementations {
            (key0).0.visit_definitions(visit, cancel)?;
            (key0).1.visit_definitions(visit, cancel)?;
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in self.constraints.values() {
            if let Some(value1) = (value0).as_ref() {
                (value1).visit_definitions(visit, cancel)?;
            }
        }
        for value0 in self.type_refs.values() {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in self.field_types.values() {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in self.expr_fields.values() {
            check_cancel(cancel)?;
            visit(value0)?;
        }
        for value0 in self.place_fields.values() {
            check_cancel(cancel)?;
            visit(value0)?;
        }
        for value0 in self.place_indexes.values() {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in self.struct_inits.values() {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in self.enum_constructors.values() {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in self.exprs.values() {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in self.callable_coercions.values() {
            (value0).0.visit_definitions(visit, cancel)?;
            (value0).1.visit_definitions(visit, cancel)?;
        }
        for value0 in self.interface_coercions.values() {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in self.locals.values() {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in self.places.values() {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in self.calls.values() {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in self.pattern_fields.values() {
            for value1 in value0 {
                check_cancel(cancel)?;
                visit(value1)?;
            }
        }
        for value0 in self.pattern_variants.values() {
            check_cancel(cancel)?;
            visit(value0)?;
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for ResolvedAssociatedConst<I> {
    type Rebind<J: DefinitionReference> = ResolvedAssociatedConst<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(ResolvedAssociatedConst {
            receiver: self.receiver.map_identities(mapper)?,
            interface: self.interface.map_identities(mapper)?,
            member: mapper.reference(&self.member)?,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.receiver.visit_definitions(visit, cancel)?;
        self.interface.visit_definitions(visit, cancel)?;
        check_cancel(cancel)?;
        visit(&self.member)?;
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for ResolvedInterfaceCoercion<I> {
    type Rebind<J: DefinitionReference> = ResolvedInterfaceCoercion<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(ResolvedInterfaceCoercion {
            implementation: self.implementation.map_identities(mapper)?,
            concrete_type: self.concrete_type.map_identities(mapper)?,
            interface_type: self.interface_type.map_identities(mapper)?,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.implementation.visit_definitions(visit, cancel)?;
        self.concrete_type.visit_definitions(visit, cancel)?;
        self.interface_type.visit_definitions(visit, cancel)?;
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for ResolvedInterfaceImplementation<I> {
    type Rebind<J: DefinitionReference> = ResolvedInterfaceImplementation<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(match self {
            Self::Upcast => ResolvedInterfaceImplementation::Upcast,
            Self::Script {
                declaration,
                arguments,
            } => ResolvedInterfaceImplementation::Script {
                declaration: mapper.reference(declaration)?,
                arguments: map_sequence(arguments, |value| (value).map_identities(mapper))?,
            },
            Self::Host => ResolvedInterfaceImplementation::Host,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        match self {
            Self::Upcast => {}
            Self::Script {
                declaration,
                arguments,
            } => {
                check_cancel(cancel)?;
                visit(declaration)?;
                for value0 in arguments {
                    (value0).visit_definitions(visit, cancel)?;
                }
            }
            Self::Host => {}
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for ResolvedHostPath<I> {
    type Rebind<J: DefinitionReference> = ResolvedHostPath<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(ResolvedHostPath {
            root: self.root,
            dynamic_arguments: self.dynamic_arguments.clone(),
            declaration: self.declaration.map_identities(mapper)?,
            contract: self.contract.map_identities(mapper)?,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.declaration.visit_definitions(visit, cancel)?;
        self.contract.visit_definitions(visit, cancel)?;
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for ResolvedHostPlacePath<I> {
    type Rebind<J: DefinitionReference> = ResolvedHostPlacePath<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(ResolvedHostPlacePath {
            root: self.root,
            dynamic_arguments: self.dynamic_arguments.clone(),
            declaration: self.declaration.map_identities(mapper)?,
            contract: self.contract.map_identities(mapper)?,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.declaration.visit_definitions(visit, cancel)?;
        self.contract.visit_definitions(visit, cancel)?;
        Ok(())
    }
}
