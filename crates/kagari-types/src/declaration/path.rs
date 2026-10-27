//! Portable manual paths consume the original shared schema and semantic types.
use crate::{
    declaration::{
        TypeDeclaration, TypeDef, TypeDefKind, fingerprint,
        host::{HostTypeOwnership, PathAccess},
        verify,
    },
    ty::Ty,
    visibility::Visibility,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind,
        mapping::DefinitionMappingError,
        scoped::DefinitionReferences,
        table::{DefinitionId, DefinitionTable},
    },
};
use std::collections::{BTreeMap, BTreeSet};

/// An explicit manual path over shared external schemas, without Rust addresses.
///
/// For `player.stats.hp`, `root` names Player and `segments` contains the Stats
/// and hp field IDs. `schema_epoch` pins the application's object schema;
/// `access` cannot exceed any traversed declaration. Dynamic indexes refer to
/// once-evaluated caller arguments by their declared slots.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HostPathDeclaration {
    pub root: DefinitionId,
    pub segments: Vec<HostPathSegmentDeclaration>,
    pub access: PathAccess,
    pub schema_epoch: u64,
}

impl HostPathDeclaration {
    /// Resolve a complete path against its shared declaration closure.
    /// Returns invalid identity, access, disconnected type, incomplete external
    /// closure or bounded shape errors before any runtime adapter can run.
    pub fn contract(
        &self,
        types: &[TypeDeclaration],
        definitions: &DefinitionTable,
    ) -> Result<HostPathContract, DefinitionMappingError> {
        resolve(self, types, definitions)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HostPathSegmentDeclaration {
    Field(DefinitionId),
    Index(HostIndexSegmentDeclaration),
    Virtual(HostVirtualSegmentDeclaration),
}

/// Portable contract for one dynamic index step; runtime type slots are resolved
/// only when a host binds the declaration.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HostIndexSegmentDeclaration {
    pub slot: u32,
    pub collection: Ty,
    pub index: Ty,
    pub result: Ty,
    pub access: PathAccess,
}

impl HostIndexSegmentDeclaration {
    pub fn validate(&self, definitions: &DefinitionTable) -> Result<(), DefinitionMappingError> {
        if self.slot >= 4096
            || self.access == PathAccess::None
            || (matches!(self.access, PathAccess::ReadWrite)
                && read_only_collection(&self.collection))
        {
            return Err(DefinitionMappingError::InvalidContract);
        }
        value_valid(&self.collection, definitions)?;
        value_valid(&self.index, definitions)?;
        value_valid(&self.result, definitions)
    }
}

/// Portable contract for a host-defined virtual step.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HostVirtualSegmentDeclaration {
    pub name: String,
    pub result: Ty,
    pub access: PathAccess,
}

impl HostVirtualSegmentDeclaration {
    pub fn validate(&self, definitions: &DefinitionTable) -> Result<(), DefinitionMappingError> {
        if self.name.is_empty() || self.name.len() > 4096 || self.access == PathAccess::None {
            return Err(DefinitionMappingError::InvalidContract);
        }
        value_valid(&self.result, definitions)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostPathInput {
    Field {
        owner: Ty,
    },
    Index {
        slot: u64,
        collection: Ty,
        index: Ty,
    },
    Virtual {
        name: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostPathSegmentContract {
    pub input: HostPathInput,
    pub result: Ty,
    pub access: PathAccess,
    pub member_fingerprint: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostPathContract {
    pub root_fingerprint: u64,
    pub result: Ty,
    pub schema_epoch: u64,
    pub access: PathAccess,

    pub segments: Vec<HostPathSegmentContract>,
}

impl HostPathContract {
    /// Fingerprint the resolved value-only contract in its identity scope.
    /// This is a compatibility key; it grants no execution or Rust access by itself.
    pub fn fingerprint(
        &self,
        definitions: &DefinitionTable,
    ) -> Result<u64, DefinitionMappingError> {
        if self.segments.is_empty()
            || self.access == PathAccess::None
            || self
                .segments
                .last()
                .is_none_or(|segment| segment.result != self.result)
        {
            return Err(DefinitionMappingError::InvalidContract);
        }
        let mut encoded = Fingerprint::new();
        encoded.number(self.root_fingerprint);
        encoded.number(fingerprint::value(
            &self.result,
            definitions,
            &Default::default(),
        )?);
        encoded.number(self.schema_epoch);
        encoded.access(self.access);

        encoded.number(self.segments.len() as u64);
        for segment in &self.segments {
            if !allows(segment.access, self.access) {
                return Err(DefinitionMappingError::InvalidContract);
            }
            match &segment.input {
                HostPathInput::Field { owner } => {
                    encoded.bytes(&[0]);
                    encoded.number(fingerprint::value(owner, definitions, &Default::default())?);
                }
                HostPathInput::Index {
                    slot,
                    collection,
                    index,
                } => {
                    if matches!(segment.access, PathAccess::ReadWrite)
                        && read_only_collection(collection)
                    {
                        return Err(DefinitionMappingError::InvalidContract);
                    }
                    encoded.bytes(&[1]);
                    encoded.number(*slot);
                    encoded.number(fingerprint::value(
                        collection,
                        definitions,
                        &Default::default(),
                    )?);
                    encoded.number(fingerprint::value(index, definitions, &Default::default())?);
                }
                HostPathInput::Virtual { name } => {
                    encoded.bytes(&[2]);
                    encoded.number(name.len() as u64);
                    encoded.bytes(name.as_bytes());
                }
            }
            encoded.number(fingerprint::value(
                &segment.result,
                definitions,
                &Default::default(),
            )?);
            encoded.access(segment.access);
            encoded.number(segment.member_fingerprint);
        }
        Ok(encoded.0)
    }
}

fn resolve(
    declaration: &HostPathDeclaration,
    types: &[TypeDeclaration],
    definitions: &DefinitionTable,
) -> Result<HostPathContract, DefinitionMappingError> {
    let cancel = CancellationToken::default();
    if declaration.segments.len() > 256 {
        return Err(DefinitionMappingError::LimitExceeded);
    }
    let mut owners = BTreeMap::new();
    for record in types {
        if owners
            .insert(record.declaration, &record.definition)
            .is_some()
            || !verify::type_definition_valid(
                &record.definition,
                record.declaration,
                &cancel,
                definitions,
            )
        {
            return Err(DefinitionMappingError::InvalidContract);
        }
    }
    for record in types {
        for field in &record.definition.fields {
            external_targets(&field.ty, &owners, definitions)?;
        }
    }
    let root = owners
        .get(&declaration.root)
        .ok_or(DefinitionMappingError::InvalidContract)?;
    let TypeDefKind::Host(policy) = root.kind else {
        return Err(DefinitionMappingError::InvalidContract);
    };
    if policy.ownership != HostTypeOwnership::HostRoot
        || !allows(policy.path_access, declaration.access)
        || declaration.access == PathAccess::None
        || declaration.segments.is_empty()
    {
        return Err(DefinitionMappingError::InvalidContract);
    }
    let mut current = Ty::Host(declaration.root);
    let mut segments = Vec::with_capacity(declaration.segments.len());
    let mut parameters = BTreeMap::new();
    for step in &declaration.segments {
        let segment = match step {
            HostPathSegmentDeclaration::Field(id) => {
                let Ty::Host(owner) = current else {
                    return Err(DefinitionMappingError::InvalidContract);
                };
                let definition = owners
                    .get(&owner)
                    .ok_or(DefinitionMappingError::InvalidContract)?;
                let (slot, field) = definition
                    .fields
                    .iter()
                    .enumerate()
                    .find(|(_, field)| {
                        definitions.lookup_child(owner, DefinitionKind::Field, &field.name, 0)
                            == Some(*id)
                    })
                    .ok_or(DefinitionMappingError::InvalidContract)?;
                let policy = field
                    .host_policy
                    .ok_or(DefinitionMappingError::InvalidContract)?;
                if field.visibility != Visibility::Public
                    || !policy.readable
                    || !allows(policy.path_access, declaration.access)
                {
                    return Err(DefinitionMappingError::InvalidContract);
                }
                HostPathSegmentContract {
                    input: HostPathInput::Field {
                        owner: current.clone(),
                    },
                    result: field.ty.clone(),
                    access: policy.path_access,
                    member_fingerprint: fingerprint::field(
                        owner,
                        definition,
                        slot,
                        &field.ty,
                        definitions,
                        &cancel,
                    )?,
                }
            }
            HostPathSegmentDeclaration::Index(index) => {
                index.validate(definitions)?;
                external_targets(&index.index, &owners, definitions)?;
                if parameters
                    .insert(index.slot, index.index.clone())
                    .is_some_and(|previous| previous != index.index)
                    || current != index.collection
                    || !allows(index.access, declaration.access)
                {
                    return Err(DefinitionMappingError::InvalidContract);
                }
                HostPathSegmentContract {
                    input: HostPathInput::Index {
                        slot: u64::from(index.slot),
                        collection: index.collection.clone(),
                        index: index.index.clone(),
                    },
                    result: index.result.clone(),
                    access: index.access,
                    member_fingerprint: 0,
                }
            }
            HostPathSegmentDeclaration::Virtual(step) => {
                step.validate(definitions)?;
                if !allows(step.access, declaration.access) {
                    return Err(DefinitionMappingError::InvalidContract);
                }
                HostPathSegmentContract {
                    input: HostPathInput::Virtual {
                        name: step.name.clone(),
                    },
                    result: step.result.clone(),
                    access: step.access,
                    member_fingerprint: 0,
                }
            }
        };
        value_valid(&segment.result, definitions)?;
        external_targets(&segment.result, &owners, definitions)?;
        current = segment.result.clone();
        segments.push(segment);
    }
    if parameters
        .keys()
        .copied()
        .enumerate()
        .any(|(slot, actual)| actual as usize != slot)
    {
        return Err(DefinitionMappingError::InvalidContract);
    }
    Ok(HostPathContract {
        root_fingerprint: fingerprint::schema(declaration.root, root, definitions, &cancel)?,
        result: current,
        schema_epoch: declaration.schema_epoch,
        access: declaration.access,
        segments,
    })
}

fn value_valid(ty: &Ty, definitions: &DefinitionTable) -> Result<(), DefinitionMappingError> {
    if !verify::concrete_type_valid(ty, &Default::default(), definitions)
        || !ty.is_concrete()
        || ty.contains_reference()
    {
        return Err(DefinitionMappingError::InvalidContract);
    }
    Ok(())
}

fn external_targets(
    ty: &Ty,
    owners: &BTreeMap<DefinitionId, &TypeDef>,
    definitions: &DefinitionTable,
) -> Result<(), DefinitionMappingError> {
    value_valid(ty, definitions)?;
    let mut references = BTreeSet::new();
    ty.visit_definition_ids(
        &mut |id| {
            references.insert(*id);
            Ok(())
        },
        &Default::default(),
    )?;
    for id in references {
        if contains_host(ty, id)
            && owners
                .get(&id)
                .is_none_or(|definition| !matches!(definition.kind, TypeDefKind::Host(_)))
        {
            return Err(DefinitionMappingError::InvalidContract);
        }
    }
    Ok(())
}

fn contains_host(ty: &Ty, declaration: DefinitionId) -> bool {
    let mut pending = vec![ty];
    while let Some(ty) = pending.pop() {
        match ty {
            Ty::Host(id) if *id == declaration => return true,
            Ty::Tuple(items) => pending.extend(items),
            Ty::Array(item) | Ty::Iter(item) | Ty::Range(item, _) => pending.push(item),

            Ty::Struct(nominal)
            | Ty::NativeObject(nominal)
            | Ty::Enum(nominal)
            | Ty::Trait(nominal) => {
                pending.extend(&nominal.arguments);
                pending.extend(nominal.associated_types.values());
            }
            Ty::Function { params, result } => {
                pending.extend(params);
                pending.push(result);
            }
            _ => {}
        }
    }
    false
}

fn allows(available: PathAccess, requested: PathAccess) -> bool {
    matches!(
        (available, requested),
        (
            PathAccess::ReadWrite,
            PathAccess::ReadOnly | PathAccess::ReadWrite
        ) | (PathAccess::ReadOnly, PathAccess::ReadOnly)
    )
}

// FNV-1a 64; all counts and integers are u64 little-endian, tags are bytes.
struct Fingerprint(u64);

impl Fingerprint {
    fn new() -> Self {
        let mut result = Self(0xcbf29ce484222325);
        result.bytes(b"kagari-host-path-v1\0");
        result
    }

    fn bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = (self.0 ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
        }
    }

    fn number(&mut self, value: u64) {
        self.bytes(&value.to_le_bytes());
    }

    fn access(&mut self, access: PathAccess) {
        self.bytes(&[match access {
            PathAccess::None => 0,
            PathAccess::ReadOnly => 1,
            PathAccess::ReadWrite => 2,
        }]);
    }
}

fn read_only_collection(ty: &Ty) -> bool {
    matches!(ty, Ty::Trait(_))
}

mod mapping;
