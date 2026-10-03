//! Explicit identity conversion for the single semantic ABI type model.
use crate::types::{
    AbiType, NominalAbiType,
    substitution::{MAX_TYPE_DEPTH, MAX_TYPE_NODES, TypeTransformError},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        reference::DefinitionReference,
        table::{
            DefinitionId, DefinitionTable, DefinitionTableBuilder, DefinitionTableError,
            wire::{MAX_PORTABLE_IDENTITY_RECORDS, PortableDefinitionRef, PortableDefinitionTable},
        },
    },
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    error::Error,
    fmt,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityTransformError {
    Identity(DefinitionTableError),
    Type(TypeTransformError),
}

impl fmt::Display for IdentityTransformError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Identity(error) => error.fmt(formatter),
            Self::Type(error) => write!(formatter, "invalid ABI identity conversion: {error:?}"),
        }
    }
}

impl Error for IdentityTransformError {}

impl From<DefinitionTableError> for IdentityTransformError {
    fn from(error: DefinitionTableError) -> Self {
        Self::Identity(error)
    }
}

impl From<TypeTransformError> for IdentityTransformError {
    fn from(error: TypeTransformError) -> Self {
        Self::Type(error)
    }
}

impl<I: DefinitionReference> AbiType<I> {
    /// Convert every owner/member reference, preserving ordered type arguments.
    /// The mapper must resolve its source context and validate the target scope.
    pub fn map_definitions<J: DefinitionReference>(
        &self,
        convert: &mut impl FnMut(&I) -> Result<J, IdentityTransformError>,
        cancel: &CancellationToken,
    ) -> Result<AbiType<J>, IdentityTransformError> {
        if !self.within_wire_limits() {
            return Err(TypeTransformError::LimitExceeded.into());
        }
        Mapper {
            convert,
            cancel,
            remaining: MAX_TYPE_NODES,
        }
        .ty(self, 1)
    }

    pub fn definition_references(
        &self,
        cancel: &CancellationToken,
    ) -> Result<Vec<&I>, IdentityTransformError> {
        if !self.within_wire_limits() {
            return Err(TypeTransformError::LimitExceeded.into());
        }
        let mut references = Vec::new();
        let mut pending = vec![self];
        while let Some(ty) = pending.pop() {
            cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
            match ty {
                Self::Host(id) | Self::SelfType(id) | Self::Parameter { owner: id, .. } => {
                    references.push(id)
                }
                Self::Struct(ty) | Self::NativeObject(ty) | Self::Enum(ty) | Self::Trait(ty) => {
                    references.push(&ty.declaration);
                    references.extend(ty.associated_types.keys());
                    pending.extend(&ty.arguments);
                    pending.extend(ty.associated_types.values());
                }
                Self::Projection {
                    arguments,
                    receiver,
                    interface,
                    member,
                } => {
                    references.extend([member, &interface.declaration]);
                    references.extend(interface.associated_types.keys());
                    pending.push(receiver);
                    pending.extend(arguments);
                    pending.extend(&interface.arguments);
                    pending.extend(interface.associated_types.values());
                }
                Self::Tuple(types) | Self::StandardEnum { args: types, .. } => {
                    pending.extend(types)
                }
                Self::Array(ty, _) | Self::Set(ty, _) | Self::Range(ty, _) | Self::Iter(ty) => {
                    pending.push(ty)
                }
                Self::Map { key, value, .. } => pending.extend([key.as_ref(), value.as_ref()]),
                Self::Function { params, result } => {
                    pending.extend(params);
                    pending.push(result);
                }
                Self::Builtin(_) => {}
            }
        }
        Ok(references)
    }
}

struct Mapper<'a, F> {
    convert: &'a mut F,
    cancel: &'a CancellationToken,
    remaining: usize,
}

impl<F> Mapper<'_, F> {
    fn ty<I: DefinitionReference, J: DefinitionReference>(
        &mut self,
        ty: &AbiType<I>,
        depth: usize,
    ) -> Result<AbiType<J>, IdentityTransformError>
    where
        F: FnMut(&I) -> Result<J, IdentityTransformError>,
    {
        self.cancel
            .check()
            .map_err(|_| TypeTransformError::Cancelled)?;
        if depth > MAX_TYPE_DEPTH || self.remaining == 0 {
            return Err(TypeTransformError::LimitExceeded.into());
        }
        self.remaining -= 1;
        Ok(match ty {
            AbiType::Host(id) => AbiType::Host((self.convert)(id)?),
            AbiType::SelfType(id) => AbiType::SelfType((self.convert)(id)?),
            AbiType::Parameter { owner, position } => AbiType::Parameter {
                owner: (self.convert)(owner)?,
                position: *position,
            },
            AbiType::Builtin(value) => AbiType::Builtin(*value),
            AbiType::Tuple(types) => AbiType::Tuple(self.many(types, depth + 1)?),
            AbiType::Function { params, result } => AbiType::Function {
                params: self.many(params, depth + 1)?,
                result: Box::new(self.ty(result, depth + 1)?),
            },
            AbiType::Array(ty, access) => {
                AbiType::Array(Box::new(self.ty(ty, depth + 1)?), *access)
            }
            AbiType::Set(ty, access) => AbiType::Set(Box::new(self.ty(ty, depth + 1)?), *access),
            AbiType::Map { key, value, access } => AbiType::Map {
                key: Box::new(self.ty(key, depth + 1)?),
                value: Box::new(self.ty(value, depth + 1)?),
                access: *access,
            },
            AbiType::Iter(ty) => AbiType::Iter(Box::new(self.ty(ty, depth + 1)?)),
            AbiType::Range(ty, kind) => AbiType::Range(Box::new(self.ty(ty, depth + 1)?), *kind),
            AbiType::Struct(ty) => AbiType::Struct(self.nominal(ty, depth)?),
            AbiType::NativeObject(ty) => AbiType::NativeObject(self.nominal(ty, depth)?),
            AbiType::Enum(ty) => AbiType::Enum(self.nominal(ty, depth)?),
            AbiType::Trait(ty) => AbiType::Trait(self.nominal(ty, depth)?),
            AbiType::StandardEnum { kind, args } => AbiType::StandardEnum {
                kind: *kind,
                args: self.many(args, depth + 1)?,
            },
            AbiType::Projection {
                arguments,
                receiver,
                interface,
                member,
            } => AbiType::Projection {
                arguments: self.many(arguments, depth + 1)?,
                receiver: Box::new(self.ty(receiver, depth + 1)?),
                interface: Box::new(self.nominal(interface, depth)?),
                member: (self.convert)(member)?,
            },
        })
    }

    fn many<I: DefinitionReference, J: DefinitionReference>(
        &mut self,
        types: &[AbiType<I>],
        depth: usize,
    ) -> Result<Vec<AbiType<J>>, IdentityTransformError>
    where
        F: FnMut(&I) -> Result<J, IdentityTransformError>,
    {
        types.iter().map(|ty| self.ty(ty, depth)).collect()
    }

    fn nominal<I: DefinitionReference, J: DefinitionReference>(
        &mut self,
        ty: &NominalAbiType<I>,
        depth: usize,
    ) -> Result<NominalAbiType<J>, IdentityTransformError>
    where
        F: FnMut(&I) -> Result<J, IdentityTransformError>,
    {
        let mut associated_types = BTreeMap::new();
        for (member, output) in &ty.associated_types {
            if associated_types
                .insert((self.convert)(member)?, self.ty(output, depth + 1)?)
                .is_some()
            {
                return Err(TypeTransformError::InvalidContract.into());
            }
        }
        Ok(NominalAbiType {
            declaration: (self.convert)(&ty.declaration)?,
            arguments: self.many(&ty.arguments, depth + 1)?,
            associated_types,
        })
    }
}

/// A portable type set contains local references and the exact identities needed
/// to interpret them. Scoped process table numbers have no Serialize implementation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortableTypes {
    definitions: PortableDefinitionTable,
    #[serde(deserialize_with = "crate::decode_limits::table")]
    types: Vec<AbiType<PortableDefinitionRef>>,
}

#[derive(Debug, Clone)]
pub struct DecodedTypes {
    pub definitions: DefinitionTable,
    pub types: Vec<AbiType<DefinitionId>>,
}

impl PortableTypes {
    pub fn encode(
        definitions: &DefinitionTable,
        types: &[AbiType<DefinitionId>],
        cancel: &CancellationToken,
    ) -> Result<Self, IdentityTransformError> {
        if types.len() > MAX_PORTABLE_IDENTITY_RECORDS {
            return Err(DefinitionTableError::PortableLimit.into());
        }
        let mut references = HashSet::new();
        for ty in types {
            references.extend(ty.definition_references(cancel)?.into_iter().copied());
            if references.len() > MAX_PORTABLE_IDENTITY_RECORDS {
                return Err(DefinitionTableError::PortableLimit.into());
            }
        }
        let encoding = definitions.encode(references)?;
        let types = types
            .iter()
            .map(|ty| {
                ty.map_definitions(
                    &mut |id| encoding.reference(*id).map_err(Into::into),
                    cancel,
                )
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            definitions: encoding.table,
            types,
        })
    }

    pub fn decode(
        self,
        cancel: &CancellationToken,
    ) -> Result<DecodedTypes, IdentityTransformError> {
        cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
        let decoding = self.definitions.decode()?;
        let types = self
            .types
            .iter()
            .map(|ty| {
                ty.map_definitions(
                    &mut |reference| decoding.resolve(*reference).map_err(Into::into),
                    cancel,
                )
            })
            .collect::<Result<_, _>>()?;
        Ok(DecodedTypes {
            definitions: decoding.table,
            types,
        })
    }
}

impl DecodedTypes {
    pub fn import_into(
        &self,
        target: &mut DefinitionTableBuilder,
        cancel: &CancellationToken,
    ) -> Result<Vec<AbiType<DefinitionId>>, IdentityTransformError> {
        let mut references = HashSet::new();
        for ty in &self.types {
            references.extend(ty.definition_references(cancel)?.into_iter().copied());
        }
        let remap = target.import(&self.definitions, references)?;
        self.types
            .iter()
            .map(|ty| ty.map_definitions(&mut |id| remap.map(*id).map_err(Into::into), cancel))
            .collect()
    }
}

#[cfg(test)]
mod tests;
