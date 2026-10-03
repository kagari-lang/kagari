//! Check trait applications against declarations in the linked dependency closure.
//! Local validation checks binder ownership and wire shape; this pass checks
//! referenced arity and members without an implicit standard-library catalog.

use crate::{
    callable::CallableImplementation,
    layout::{EnumLayout, StructLayout},
    slots::SemanticSlots,
    types::{
        AbiType, ConstraintAbi, FunctionAbi, GenericBoundAbi, NominalAbiType, PublicAbiItem,
        TraitAbi, TraitContract, TypeAbi, TypeAbiKind,
        substitution::{MAX_TYPE_NODES, TypeTransformError},
    },
};
use kagari_common::{cancellation::CancellationToken, identity::DefinitionPath};

pub struct ApplicationValidator<'a, F, G> {
    lookup: F,
    storage: G,
    cancel: &'a CancellationToken,
}

impl<'a, 'declaration, F, G> ApplicationValidator<'a, F, G>
where
    F: Fn(&DefinitionPath) -> Option<&'declaration TraitAbi>,
    G: Fn(&DefinitionPath) -> Option<&'declaration TypeAbi>,
{
    pub fn new(cancel: &'a CancellationToken, lookup: F, storage: G) -> Self {
        Self {
            lookup,
            storage,
            cancel,
        }
    }

    fn contract(
        &self,
        applied: &NominalAbiType,
    ) -> Result<&'declaration TraitAbi, TypeTransformError> {
        self.cancel
            .check()
            .map_err(|_| TypeTransformError::Cancelled)?;
        let record =
            (self.lookup)(&applied.declaration).ok_or(TypeTransformError::InvalidContract)?;
        if record.generic_params.len() != applied.arguments.len()
            || applied.associated_types.keys().any(|id| {
                !record
                    .associated_types
                    .iter()
                    .any(|member| member.declaration == *id && member.generic_params.is_empty())
            })
        {
            return Err(TypeTransformError::InvalidContract);
        }
        Ok(record)
    }

    pub fn validate_type(&self, ty: &AbiType) -> Result<(), TypeTransformError> {
        let mut pending = vec![ty];
        let mut remaining = MAX_TYPE_NODES;
        while let Some(ty) = pending.pop() {
            self.cancel
                .check()
                .map_err(|_| TypeTransformError::Cancelled)?;
            if remaining == 0 {
                return Err(TypeTransformError::LimitExceeded);
            }
            remaining -= 1;
            match ty {
                AbiType::Projection {
                    receiver,
                    interface,
                    member,
                    arguments,
                } => {
                    let record = self.contract(interface)?;
                    if !record.associated_types.iter().any(|definition| {
                        definition.declaration == *member
                            && definition.generic_params.len() == arguments.len()
                    }) {
                        return Err(TypeTransformError::InvalidContract);
                    }
                    pending.push(receiver);
                    pending.extend(&interface.arguments);
                    pending.extend(interface.associated_types.values());
                    pending.extend(arguments);
                }
                AbiType::Trait(applied) => {
                    self.contract(applied)?;
                    pending.extend(&applied.arguments);
                    pending.extend(applied.associated_types.values());
                }
                AbiType::NativeObject(applied) => {
                    let record = (self.storage)(&applied.declaration)
                        .ok_or(TypeTransformError::InvalidContract)?;
                    let TypeAbiKind::NativeStorage(layout) = record.kind else {
                        return Err(TypeTransformError::InvalidContract);
                    };
                    if record.generic_params.len() != applied.arguments.len()
                        || !applied.associated_types.is_empty()
                        || !layout.valid_parameters(applied.arguments.len())
                    {
                        return Err(TypeTransformError::InvalidContract);
                    }
                    pending.extend(&applied.arguments);
                }
                AbiType::Struct(applied) | AbiType::Enum(applied) => {
                    pending.extend(&applied.arguments);
                    pending.extend(applied.associated_types.values());
                }
                AbiType::Tuple(items) | AbiType::StandardEnum { args: items, .. } => {
                    pending.extend(items)
                }
                AbiType::Function { params, result } => {
                    pending.extend(params);
                    pending.push(result);
                }
                AbiType::Array(item, _)
                | AbiType::Set(item, _)
                | AbiType::Iter(item)
                | AbiType::Range(item, _) => pending.push(item),
                AbiType::Map { key, value, .. } => pending.extend([key.as_ref(), value.as_ref()]),
                AbiType::Builtin(_)
                | AbiType::Host(_)
                | AbiType::Parameter { .. }
                | AbiType::SelfType(_) => {}
            }
            if pending.len() > remaining {
                return Err(TypeTransformError::LimitExceeded);
            }
        }
        Ok(())
    }

    /// The nominal itself may be an aggregate; callers validate its kind locally.
    pub fn nominal_arguments(&self, ty: &NominalAbiType) -> Result<(), TypeTransformError> {
        self.types(ty.arguments.iter().chain(ty.associated_types.values()))
    }

    pub fn trait_application(&self, ty: &NominalAbiType) -> Result<(), TypeTransformError> {
        self.contract(ty)?;
        self.nominal_arguments(ty)
    }

    pub fn types<'ty>(
        &self,
        types: impl IntoIterator<Item = &'ty AbiType>,
    ) -> Result<(), TypeTransformError> {
        for ty in types {
            self.validate_type(ty)?;
        }
        Ok(())
    }

    fn constraints(&self, constraints: &[ConstraintAbi]) -> Result<(), TypeTransformError> {
        for constraint in constraints {
            if let ConstraintAbi::Trait(applied) = constraint {
                self.trait_application(applied)?;
            }
        }
        Ok(())
    }

    pub fn bounds(&self, bounds: &[GenericBoundAbi]) -> Result<(), TypeTransformError> {
        for bound in bounds {
            self.validate_type(&bound.ty)?;
            self.constraints(&bound.constraints)?;
        }
        Ok(())
    }

    pub fn function(&self, function: &FunctionAbi) -> Result<(), TypeTransformError> {
        if let CallableImplementation::NativeDefault(application) = &function.implementation {
            self.types(&application.arguments)?;
        }
        self.bounds(&function.bounds)?;
        self.types(function.params.iter().map(|p| &p.ty))?;
        self.validate_type(&function.return_type)
    }

    fn declaration(&self, record: &TraitAbi) -> Result<(), TypeTransformError> {
        self.bounds(&record.bounds)?;
        for parent in &record.supertraits {
            self.trait_application(parent)?;
        }
        for method in &record.methods {
            self.function(method)?;
        }
        self.types(record.associated_consts.iter().map(|member| &member.ty))?;
        for member in &record.associated_types {
            self.bounds(&member.parameter_bounds)?;
            self.constraints(&member.bounds)?;
        }
        Ok(())
    }

    pub fn declarations(
        &self,
        items: &[PublicAbiItem],
        private: &[TraitContract],
    ) -> Result<(), TypeTransformError> {
        for item in items {
            self.cancel
                .check()
                .map_err(|_| TypeTransformError::Cancelled)?;
            match item {
                PublicAbiItem::Function(function) => self.function(function)?,
                PublicAbiItem::Const(value) => self.validate_type(&value.ty)?,
                PublicAbiItem::Type(record) => {
                    self.bounds(&record.bounds)?;
                    self.types(record.fields.iter().map(|field| &field.ty))?;
                    self.types(record.variants.iter().flat_map(|variant| &variant.payload))?;
                }
                PublicAbiItem::Trait(record) => self.declaration(record)?,
                PublicAbiItem::InterfaceTable(table) => {
                    self.validate_type(&table.for_type)?;
                    self.validate_type(&table.trait_type)?;
                    self.bounds(&table.bounds)?;
                    for method in &table.methods {
                        self.function(method)?;
                    }
                    self.types(table.associated_consts.iter().map(|member| &member.ty))?;
                    for family in &table.associated_type_families {
                        self.validate_type(&family.value)?;
                        self.bounds(&family.bounds)?;
                    }
                }
            }
        }
        for record in private {
            self.declaration(&record.abi)?;
        }
        Ok(())
    }

    pub fn layouts(
        &self,
        structures: &[StructLayout],
        enumerations: &[EnumLayout],
    ) -> Result<(), TypeTransformError> {
        for layout in structures {
            self.types(&layout.arguments)?;
            self.types(layout.fields.iter().map(|field| &field.ty))?;
        }
        for layout in enumerations {
            self.types(&layout.arguments)?;
            self.types(layout.variants.iter().flat_map(|variant| &variant.payload))?;
        }
        Ok(())
    }

    pub fn slots(&self, slots: &SemanticSlots) -> Result<(), TypeTransformError> {
        if let Some(body) = &slots.generic {
            self.bounds(&body.bounds)?;
        }
        if let Some(required) = &slots.protocol_adapter {
            self.validate_type(&required.receiver)?;
            self.contract(&required.interface)?;
            self.types(&required.arguments)?;
        }
        self.types(
            slots
                .params
                .values()
                .chain(slots.result.iter())
                .chain(slots.locals.values())
                .chain(slots.registers.values()),
        )
    }
}

#[cfg(test)]
mod tests;
