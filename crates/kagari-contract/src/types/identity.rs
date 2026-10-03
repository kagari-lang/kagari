//! Explicit identity conversion for the single semantic ABI type model.
use crate::types::{
    NominalTy, Ty,
    substitution::{MAX_TYPE_DEPTH, MAX_TYPE_NODES, TypeTransformError},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, DefinitionRecord},
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

impl From<IdentityTransformError> for DefinitionMappingError {
    fn from(error: IdentityTransformError) -> Self {
        match error {
            IdentityTransformError::Identity(error) => Self::Identity(error),
            IdentityTransformError::Type(TypeTransformError::Cancelled) => Self::Cancelled,
            IdentityTransformError::Type(TypeTransformError::LimitExceeded) => Self::LimitExceeded,
            IdentityTransformError::Type(TypeTransformError::InvalidContract) => {
                Self::InvalidContract
            }
        }
    }
}

impl From<DefinitionMappingError> for IdentityTransformError {
    fn from(error: DefinitionMappingError) -> Self {
        match error {
            DefinitionMappingError::Identity(error) => Self::Identity(error),
            DefinitionMappingError::Cancelled => TypeTransformError::Cancelled.into(),
            DefinitionMappingError::LimitExceeded => TypeTransformError::LimitExceeded.into(),
            DefinitionMappingError::InvalidContract => TypeTransformError::InvalidContract.into(),
        }
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for Ty<I> {
    type Rebind<J: DefinitionReference> = Ty<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Ty<J>, DefinitionMappingError> {
        let cancel = mapper.cancellation().clone();
        self.map_definitions(
            &mut |id| mapper.reference(id).map_err(IdentityTransformError::from),
            &cancel,
        )
        .map_err(DefinitionMappingError::from)
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        for id in self
            .definition_references(cancel)
            .map_err(DefinitionMappingError::from)?
        {
            visit(id)?;
        }
        Ok(())
    }
}

impl<I: DefinitionReference> Ty<I> {
    /// Convert every owner/member reference, preserving ordered type arguments.
    /// The mapper must resolve its source context and validate the target scope.
    pub fn map_definitions<J: DefinitionReference>(
        &self,
        convert: &mut impl FnMut(&I) -> Result<J, IdentityTransformError>,
        cancel: &CancellationToken,
    ) -> Result<Ty<J>, IdentityTransformError> {
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
        ty: &Ty<I>,
        depth: usize,
    ) -> Result<Ty<J>, IdentityTransformError>
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
            Ty::Host(id) => Ty::Host((self.convert)(id)?),
            Ty::SelfType(id) => Ty::SelfType((self.convert)(id)?),
            Ty::Parameter { owner, position } => Ty::Parameter {
                owner: (self.convert)(owner)?,
                position: *position,
            },
            Ty::Builtin(value) => Ty::Builtin(*value),
            Ty::Tuple(types) => Ty::Tuple(self.many(types, depth + 1)?),
            Ty::Function { params, result } => Ty::Function {
                params: self.many(params, depth + 1)?,
                result: Box::new(self.ty(result, depth + 1)?),
            },
            Ty::Array(ty, access) => Ty::Array(Box::new(self.ty(ty, depth + 1)?), *access),
            Ty::Set(ty, access) => Ty::Set(Box::new(self.ty(ty, depth + 1)?), *access),
            Ty::Map { key, value, access } => Ty::Map {
                key: Box::new(self.ty(key, depth + 1)?),
                value: Box::new(self.ty(value, depth + 1)?),
                access: *access,
            },
            Ty::Iter(ty) => Ty::Iter(Box::new(self.ty(ty, depth + 1)?)),
            Ty::Range(ty, kind) => Ty::Range(Box::new(self.ty(ty, depth + 1)?), *kind),
            Ty::Struct(ty) => Ty::Struct(self.nominal(ty, depth)?),
            Ty::NativeObject(ty) => Ty::NativeObject(self.nominal(ty, depth)?),
            Ty::Enum(ty) => Ty::Enum(self.nominal(ty, depth)?),
            Ty::Trait(ty) => Ty::Trait(self.nominal(ty, depth)?),
            Ty::StandardEnum { kind, args } => Ty::StandardEnum {
                kind: *kind,
                args: self.many(args, depth + 1)?,
            },
            Ty::Projection {
                arguments,
                receiver,
                interface,
                member,
            } => Ty::Projection {
                arguments: self.many(arguments, depth + 1)?,
                receiver: Box::new(self.ty(receiver, depth + 1)?),
                interface: Box::new(self.nominal(interface, depth)?),
                member: (self.convert)(member)?,
            },
        })
    }

    fn many<I: DefinitionReference, J: DefinitionReference>(
        &mut self,
        types: &[Ty<I>],
        depth: usize,
    ) -> Result<Vec<Ty<J>>, IdentityTransformError>
    where
        F: FnMut(&I) -> Result<J, IdentityTransformError>,
    {
        types.iter().map(|ty| self.ty(ty, depth)).collect()
    }

    fn nominal<I: DefinitionReference, J: DefinitionReference>(
        &mut self,
        ty: &NominalTy<I>,
        depth: usize,
    ) -> Result<NominalTy<J>, IdentityTransformError>
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
        Ok(NominalTy {
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
    types: Vec<Ty<PortableDefinitionRef>>,
}

#[derive(Debug, Clone)]
pub struct DecodedTypes {
    pub definitions: DefinitionTable,
    pub types: Vec<Ty<DefinitionId>>,
}

impl PortableTypes {
    pub fn encode(
        definitions: &DefinitionTable,
        types: &[Ty<DefinitionId>],
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
    ) -> Result<Vec<Ty<DefinitionId>>, IdentityTransformError> {
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
