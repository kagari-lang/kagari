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
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel, map_sequence},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};

impl RemapDefinitions for ConstraintTarget {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(match self {
            Self::Standard(field0) => ConstraintTarget::Standard(*(field0)),
            Self::Trait(field0) => ConstraintTarget::Trait((field0).remap_definitions(mapper)?),
        })
    }
}

impl DefinitionReferences for ConstraintTarget {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        match self {
            Self::Standard(_) => {}
            Self::Trait(field0) => {
                (field0).visit_definition_ids(visit, cancel)?;
            }
        }
        Ok(())
    }
}

impl RemapDefinitions for TypeTarget {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
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
}

impl DefinitionReferences for TypeTarget {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
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

impl RemapDefinitions for ResolvedTypeRef {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(ResolvedTypeRef {
            ty: self.ty.remap_definitions(mapper)?,
            target: self
                .target
                .as_ref()
                .map(|value| (value).remap_definitions(mapper))
                .transpose()?,
        })
    }
}

impl DefinitionReferences for ResolvedTypeRef {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.ty.visit_definition_ids(visit, cancel)?;
        if let Some(value0) = self.target.as_ref() {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}

impl RemapDefinitions for CallTarget {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
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
                interface: (interface).remap_definitions(mapper)?,
            },
        })
    }
}

impl DefinitionReferences for CallTarget {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
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
                (interface).visit_definition_ids(visit, cancel)?;
            }
        }
        Ok(())
    }
}

impl RemapDefinitions for ResolvedCall {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(ResolvedCall {
            target: self.target.remap_definitions(mapper)?,
            receiver: self.receiver,
            type_arguments: map_sequence(&self.type_arguments, |value| {
                (value).remap_definitions(mapper)
            })?,
            signature: self
                .signature
                .as_ref()
                .map(|value| (value).remap_definitions(mapper))
                .transpose()?,
        })
    }
}

impl DefinitionReferences for ResolvedCall {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.target.visit_definition_ids(visit, cancel)?;
        for value0 in &self.type_arguments {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        if let Some(value0) = self.signature.as_ref() {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}

impl RemapDefinitions for ResolvedStructInit {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
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
}

impl DefinitionReferences for ResolvedStructInit {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
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

impl RemapDefinitions for ResolvedEnumConstructor {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
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
}

impl DefinitionReferences for ResolvedEnumConstructor {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        visit(&self.enumeration)?;
        if let Some(value0) = self.variant.as_ref() {
            check_cancel(cancel)?;
            visit(value0)?;
        }
        Ok(())
    }
}

impl RemapDefinitions for TraitImplementation {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(TraitImplementation {
            declaration: mapper.reference(&self.declaration)?,
            parameters: map_sequence(&self.parameters, |value| (value).remap_definitions(mapper))?,
            bounds: map_hash_entries(
                self.bounds.len(),
                self.bounds.iter().map(|(key, value)| {
                    Ok((
                        (key).remap_definitions(mapper)?,
                        map_sequence(value, |value| (value).remap_definitions(mapper))?,
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
}

impl DefinitionReferences for TraitImplementation {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        visit(&self.declaration)?;
        for value0 in &self.parameters {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for (key0, value0) in &self.bounds {
            (key0).visit_definition_ids(visit, cancel)?;
            for value1 in value0 {
                (value1).visit_definition_ids(visit, cancel)?;
            }
        }
        for key0 in self.methods.keys() {
            check_cancel(cancel)?;
            visit(key0)?;
        }
        Ok(())
    }
}

impl RemapDefinitions for ResolvedIteration {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(ResolvedIteration {
            into_interface: self.into_interface.remap_definitions(mapper)?,
            iterator: self.iterator.remap_definitions(mapper)?,
            next_interface: self.next_interface.remap_definitions(mapper)?,
            item: self.item.remap_definitions(mapper)?,
        })
    }
}

impl DefinitionReferences for ResolvedIteration {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.into_interface.visit_definition_ids(visit, cancel)?;
        self.iterator.visit_definition_ids(visit, cancel)?;
        self.next_interface.visit_definition_ids(visit, cancel)?;
        self.item.visit_definition_ids(visit, cancel)?;
        Ok(())
    }
}

impl RemapDefinitions for TypeTable {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(TypeTable {
            inference_holes: map_hash_entries(
                self.inference_holes.len(),
                self.inference_holes
                    .iter()
                    .map(|(key, value)| Ok((*(key), (value).remap_definitions(mapper)?))),
            )?,
            iterations: map_hash_entries(
                self.iterations.len(),
                self.iterations
                    .iter()
                    .map(|(key, value)| Ok((*(key), (value).remap_definitions(mapper)?))),
            )?,
            propagations: map_hash_entries(
                self.propagations.len(),
                self.propagations
                    .iter()
                    .map(|(key, value)| Ok((*key, value.remap_definitions(mapper)?))),
            )?,
            protocol_receivers: map_hash_entries(
                self.protocol_receivers.len(),
                self.protocol_receivers
                    .iter()
                    .map(|(key, value)| Ok((*(key), (value).remap_definitions(mapper)?))),
            )?,
            associated_consts: map_hash_entries(
                self.associated_consts.len(),
                self.associated_consts
                    .iter()
                    .map(|(key, value)| Ok((*(key), (value).remap_definitions(mapper)?))),
            )?,
            resolving_types: self.resolving_types.clone(),
            associated_bounds: map_hash_entries(
                self.associated_bounds.len(),
                self.associated_bounds.iter().map(|(key, value)| {
                    Ok((
                        mapper.reference(key)?,
                        map_sequence(value, |value| (value).remap_definitions(mapper))?,
                    ))
                }),
            )?,
            associated_type_parameters: map_hash_entries(
                self.associated_type_parameters.len(),
                self.associated_type_parameters.iter().map(|(key, value)| {
                    Ok((mapper.reference(key)?, (value).remap_definitions(mapper)?))
                }),
            )?,
            associated_type_families: map_hash_entries(
                self.associated_type_families.len(),
                self.associated_type_families.iter().map(|(key, value)| {
                    Ok((mapper.reference(key)?, (value).remap_definitions(mapper)?))
                }),
            )?,
            host_place_paths: map_hash_entries(
                self.host_place_paths.len(),
                self.host_place_paths
                    .iter()
                    .map(|(key, value)| Ok((*(key), (value).remap_definitions(mapper)?))),
            )?,
            host_paths: map_hash_entries(
                self.host_paths.len(),
                self.host_paths
                    .iter()
                    .map(|(key, value)| Ok((*(key), (value).remap_definitions(mapper)?))),
            )?,
            implementations: map_hash_entries(
                self.implementations.len(),
                self.implementations.iter().map(|(key, value)| {
                    Ok((
                        (
                            (key).0.remap_definitions(mapper)?,
                            (key).1.remap_definitions(mapper)?,
                        ),
                        (value).remap_definitions(mapper)?,
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
                            .map(|value| (value).remap_definitions(mapper))
                            .transpose()?,
                    ))
                }),
            )?,
            type_refs: map_hash_entries(
                self.type_refs.len(),
                self.type_refs
                    .iter()
                    .map(|(key, value)| Ok((*(key), (value).remap_definitions(mapper)?))),
            )?,
            field_types: map_hash_entries(
                self.field_types.len(),
                self.field_types
                    .iter()
                    .map(|(key, value)| Ok((*(key), (value).remap_definitions(mapper)?))),
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
                    .map(|(key, value)| Ok((*(key), (value).remap_definitions(mapper)?))),
            )?,
            place_index_writes: map_hash_entries(
                self.place_index_writes.len(),
                self.place_index_writes
                    .iter()
                    .map(|(key, (interface, method))| {
                        Ok((
                            *key,
                            (
                                interface.remap_definitions(mapper)?,
                                mapper.reference(method)?,
                            ),
                        ))
                    }),
            )?,
            struct_inits: map_hash_entries(
                self.struct_inits.len(),
                self.struct_inits
                    .iter()
                    .map(|(key, value)| Ok((*(key), (value).remap_definitions(mapper)?))),
            )?,
            enum_constructors: map_hash_entries(
                self.enum_constructors.len(),
                self.enum_constructors
                    .iter()
                    .map(|(key, value)| Ok((*(key), (value).remap_definitions(mapper)?))),
            )?,
            exprs: map_hash_entries(
                self.exprs.len(),
                self.exprs
                    .iter()
                    .map(|(key, value)| Ok((*(key), (value).remap_definitions(mapper)?))),
            )?,
            callable_coercions: map_hash_entries(
                self.callable_coercions.len(),
                self.callable_coercions.iter().map(|(key, value)| {
                    Ok((
                        *(key),
                        (
                            (value).0.remap_definitions(mapper)?,
                            (value).1.remap_definitions(mapper)?,
                        ),
                    ))
                }),
            )?,
            interface_coercions: map_hash_entries(
                self.interface_coercions.len(),
                self.interface_coercions
                    .iter()
                    .map(|(key, value)| Ok((*(key), (value).remap_definitions(mapper)?))),
            )?,
            locals: map_hash_entries(
                self.locals.len(),
                self.locals
                    .iter()
                    .map(|(key, value)| Ok((*(key), (value).remap_definitions(mapper)?))),
            )?,
            places: map_hash_entries(
                self.places.len(),
                self.places
                    .iter()
                    .map(|(key, value)| Ok((*(key), (value).remap_definitions(mapper)?))),
            )?,
            native_default_calls: self.native_default_calls.clone(),
            calls: map_hash_entries(
                self.calls.len(),
                self.calls
                    .iter()
                    .map(|(key, value)| Ok((*(key), (value).remap_definitions(mapper)?))),
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
}

impl DefinitionReferences for TypeTable {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for value0 in self.inference_holes.values() {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value in self.propagations.values() {
            value.visit_definition_ids(visit, cancel)?;
        }
        for value0 in self.iterations.values() {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in self.protocol_receivers.values() {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in self.associated_consts.values() {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for (key0, value0) in &self.associated_bounds {
            check_cancel(cancel)?;
            visit(key0)?;
            for value1 in value0 {
                (value1).visit_definition_ids(visit, cancel)?;
            }
        }
        for (key0, value0) in &self.associated_type_parameters {
            check_cancel(cancel)?;
            visit(key0)?;
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for (key0, value0) in &self.associated_type_families {
            check_cancel(cancel)?;
            visit(key0)?;
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in self.host_place_paths.values() {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in self.host_paths.values() {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for (key0, value0) in &self.implementations {
            (key0).0.visit_definition_ids(visit, cancel)?;
            (key0).1.visit_definition_ids(visit, cancel)?;
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in self.constraints.values() {
            if let Some(value1) = (value0).as_ref() {
                (value1).visit_definition_ids(visit, cancel)?;
            }
        }
        for value0 in self.type_refs.values() {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in self.field_types.values() {
            (value0).visit_definition_ids(visit, cancel)?;
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
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for (interface, method) in self.place_index_writes.values() {
            interface.visit_definition_ids(visit, cancel)?;
            visit(method)?;
        }
        for value0 in self.struct_inits.values() {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in self.enum_constructors.values() {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in self.exprs.values() {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in self.callable_coercions.values() {
            (value0).0.visit_definition_ids(visit, cancel)?;
            (value0).1.visit_definition_ids(visit, cancel)?;
        }
        for value0 in self.interface_coercions.values() {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in self.locals.values() {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in self.places.values() {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in self.calls.values() {
            (value0).visit_definition_ids(visit, cancel)?;
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

impl RemapDefinitions for ResolvedAssociatedConst {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(ResolvedAssociatedConst {
            receiver: self.receiver.remap_definitions(mapper)?,
            interface: self.interface.remap_definitions(mapper)?,
            member: mapper.reference(&self.member)?,
        })
    }
}

impl DefinitionReferences for ResolvedAssociatedConst {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.receiver.visit_definition_ids(visit, cancel)?;
        self.interface.visit_definition_ids(visit, cancel)?;
        check_cancel(cancel)?;
        visit(&self.member)?;
        Ok(())
    }
}

impl RemapDefinitions for ResolvedInterfaceCoercion {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(ResolvedInterfaceCoercion {
            implementation: self.implementation.remap_definitions(mapper)?,
            concrete_type: self.concrete_type.remap_definitions(mapper)?,
            interface_type: self.interface_type.remap_definitions(mapper)?,
        })
    }
}

impl DefinitionReferences for ResolvedInterfaceCoercion {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.implementation.visit_definition_ids(visit, cancel)?;
        self.concrete_type.visit_definition_ids(visit, cancel)?;
        self.interface_type.visit_definition_ids(visit, cancel)?;
        Ok(())
    }
}

impl RemapDefinitions for ResolvedInterfaceImplementation {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(match self {
            Self::Upcast => ResolvedInterfaceImplementation::Upcast,
            Self::Declared {
                declaration,
                arguments,
            } => ResolvedInterfaceImplementation::Declared {
                declaration: mapper.reference(declaration)?,
                arguments: map_sequence(arguments, |value| (value).remap_definitions(mapper))?,
            },
            Self::Intrinsic => ResolvedInterfaceImplementation::Intrinsic,
        })
    }
}

impl DefinitionReferences for ResolvedInterfaceImplementation {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        match self {
            Self::Upcast => {}
            Self::Declared {
                declaration,
                arguments,
            } => {
                check_cancel(cancel)?;
                visit(declaration)?;
                for value0 in arguments {
                    (value0).visit_definition_ids(visit, cancel)?;
                }
            }
            Self::Intrinsic => {}
        }
        Ok(())
    }
}

impl RemapDefinitions for ResolvedHostPath {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(ResolvedHostPath {
            root: self.root,
            dynamic_arguments: self.dynamic_arguments.clone(),
            declaration: self.declaration.remap_definitions(mapper)?,
            contract: self.contract.remap_definitions(mapper)?,
        })
    }
}

impl DefinitionReferences for ResolvedHostPath {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.declaration.visit_definition_ids(visit, cancel)?;
        self.contract.visit_definition_ids(visit, cancel)?;
        Ok(())
    }
}

impl RemapDefinitions for ResolvedHostPlacePath {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(ResolvedHostPlacePath {
            root: self.root,
            dynamic_arguments: self.dynamic_arguments.clone(),
            declaration: self.declaration.remap_definitions(mapper)?,
            contract: self.contract.remap_definitions(mapper)?,
        })
    }
}

impl DefinitionReferences for ResolvedHostPlacePath {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.declaration.visit_definition_ids(visit, cancel)?;
        self.contract.visit_definition_ids(visit, cancel)?;
        Ok(())
    }
}
