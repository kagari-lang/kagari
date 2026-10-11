//! Portable resolved path contracts shared by offline tools and runtime registration.
use crate::collection::CollectionAccess;
use crate::host_interface::{
    DefinitionPath, HostInterface, HostInterfaceError,
    type_declaration::{HostTypeOwnership, PathAccess, Visibility},
    value_type::HostValueType,
};
use kagari_common::identity::{
    reference::DefinitionReference,
    table::{DefinitionId, DefinitionTable},
};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct HostPathDeclaration<I = DefinitionPath> {
    pub root: I,
    #[serde(deserialize_with = "super::decode_limits::members")]
    pub segments: Vec<HostPathSegmentDeclaration<I>>,
    pub access: PathAccess,
    pub schema_epoch: u64,
}

impl HostPathDeclaration<DefinitionId> {
    pub fn contract(
        &self,
        interface: &HostInterface<DefinitionId>,
        definitions: &DefinitionTable,
    ) -> Result<HostPathContract<DefinitionId>, HostInterfaceError> {
        interface.path_contract(self, definitions)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub enum HostPathSegmentDeclaration<I = DefinitionPath> {
    Field(I),
    Index(HostIndexSegmentDeclaration<I>),
    Virtual(HostVirtualSegmentDeclaration<I>),
}

/// Portable contract for one dynamic index step; runtime type slots are resolved
/// only when a host binds the declaration.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct HostIndexSegmentDeclaration<I = DefinitionPath> {
    pub slot: u32,
    pub collection: HostValueType<I>,
    pub index: HostValueType<I>,
    pub result: HostValueType<I>,
    pub access: PathAccess,
}

impl HostIndexSegmentDeclaration<DefinitionId> {
    pub fn validate(&self, definitions: &DefinitionTable) -> Result<(), HostInterfaceError> {
        if self.slot >= 4096
            || self.access == PathAccess::None
            || (matches!(self.access, PathAccess::ReadWrite)
                && read_only_collection(&self.collection))
        {
            return Err(HostInterfaceError::InvalidDeclaration);
        }
        self.collection.validate_in(definitions)?;
        self.index.validate_in(definitions)?;
        self.result.validate_in(definitions)
    }
}

/// Portable contract for a host-defined virtual step.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct HostVirtualSegmentDeclaration<I = DefinitionPath> {
    pub name: String,
    pub result: HostValueType<I>,
    pub access: PathAccess,
}

impl HostVirtualSegmentDeclaration<DefinitionId> {
    pub fn validate(&self, definitions: &DefinitionTable) -> Result<(), HostInterfaceError> {
        if self.name.is_empty() || self.name.len() > 4096 || self.access == PathAccess::None {
            return Err(HostInterfaceError::InvalidDeclaration);
        }
        self.result.validate_in(definitions)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostPathInput<I = DefinitionPath> {
    Field {
        owner: HostValueType<I>,
    },
    Index {
        slot: u64,
        collection: HostValueType<I>,
        index: HostValueType<I>,
    },
    Virtual {
        name: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostPathSegmentContract<I = DefinitionPath> {
    pub input: HostPathInput<I>,
    pub result: HostValueType<I>,
    pub access: PathAccess,
    pub member_fingerprint: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostPathContract<I = DefinitionPath> {
    pub root_fingerprint: u64,
    pub result: HostValueType<I>,
    pub schema_epoch: u64,
    pub access: PathAccess,

    pub segments: Vec<HostPathSegmentContract<I>>,
}

impl HostPathContract<DefinitionId> {
    pub fn fingerprint(&self, definitions: &DefinitionTable) -> Result<u64, HostInterfaceError> {
        if self.segments.is_empty()
            || self.access == PathAccess::None
            || self
                .segments
                .last()
                .is_none_or(|segment| segment.result != self.result)
        {
            return Err(HostInterfaceError::InvalidDeclaration);
        }
        let mut encoded = Fingerprint::new();
        encoded.number(self.root_fingerprint);
        encoded.number(self.result.fingerprint(definitions)?);
        encoded.number(self.schema_epoch);
        encoded.access(self.access);

        encoded.number(self.segments.len() as u64);
        for segment in &self.segments {
            if !allows(segment.access, self.access) {
                return Err(HostInterfaceError::InvalidDeclaration);
            }
            match &segment.input {
                HostPathInput::Field { owner } => {
                    encoded.bytes(&[0]);
                    encoded.number(owner.fingerprint(definitions)?);
                }
                HostPathInput::Index {
                    slot,
                    collection,
                    index,
                } => {
                    if matches!(segment.access, PathAccess::ReadWrite)
                        && read_only_collection(collection)
                    {
                        return Err(HostInterfaceError::InvalidDeclaration);
                    }
                    encoded.bytes(&[1]);
                    encoded.number(*slot);
                    encoded.number(collection.fingerprint(definitions)?);
                    encoded.number(index.fingerprint(definitions)?);
                }
                HostPathInput::Virtual { name } => {
                    encoded.bytes(&[2]);
                    encoded.number(name.len() as u64);
                    encoded.bytes(name.as_bytes());
                }
            }
            encoded.number(segment.result.fingerprint(definitions)?);
            encoded.access(segment.access);
            encoded.number(segment.member_fingerprint);
        }
        Ok(encoded.0)
    }
}

impl HostInterface<DefinitionId> {
    /// Resolve the complete path without invoking runtime bindings.
    pub fn path_contract(
        &self,
        declaration: &HostPathDeclaration<DefinitionId>,
        definitions: &DefinitionTable,
    ) -> Result<HostPathContract<DefinitionId>, HostInterfaceError> {
        self.validate(definitions)?;
        self.resolve_path(declaration, definitions)
    }

    pub(super) fn resolve_path(
        &self,
        declaration: &HostPathDeclaration<DefinitionId>,
        definitions: &DefinitionTable,
    ) -> Result<HostPathContract<DefinitionId>, HostInterfaceError> {
        if declaration.segments.len() > 256 {
            return Err(HostInterfaceError::TooLarge);
        }
        let root = self
            .types
            .iter()
            .find(|ty| ty.id == declaration.root)
            .ok_or(HostInterfaceError::InvalidDeclaration)?;
        if root.ownership != HostTypeOwnership::HostRoot
            || !allows(root.path_access, declaration.access)
            || declaration.access == PathAccess::None
            || declaration.segments.is_empty()
        {
            return Err(HostInterfaceError::InvalidDeclaration);
        }
        let mut current = HostValueType::Opaque(root.id);
        let mut segments = Vec::with_capacity(declaration.segments.len());
        let mut dynamic_parameters = BTreeMap::<u32, HostValueType<DefinitionId>>::new();
        for step in &declaration.segments {
            let resolved = match step {
                HostPathSegmentDeclaration::Field(field_id) => {
                    let HostValueType::Opaque(owner) = &current else {
                        return Err(HostInterfaceError::InvalidDeclaration);
                    };
                    let owner = self
                        .types
                        .iter()
                        .find(|ty| &ty.id == owner)
                        .ok_or(HostInterfaceError::InvalidDeclaration)?;
                    let field = owner
                        .fields
                        .iter()
                        .find(|field| &field.id == field_id)
                        .ok_or(HostInterfaceError::InvalidDeclaration)?;
                    if field.visibility != Visibility::Public
                        || !allows(field.path_access, declaration.access)
                    {
                        return Err(HostInterfaceError::InvalidDeclaration);
                    }
                    HostPathSegmentContract {
                        input: HostPathInput::Field {
                            owner: current.clone(),
                        },
                        result: field.ty.clone(),
                        access: field.path_access,
                        member_fingerprint: field.fingerprint(definitions)?,
                    }
                }
                HostPathSegmentDeclaration::Index(index) => {
                    index.validate(definitions)?;
                    if dynamic_parameters
                        .insert(index.slot, index.index.clone())
                        .is_some_and(|existing| existing != index.index)
                    {
                        return Err(HostInterfaceError::InvalidDeclaration);
                    }
                    if current != index.collection
                        || !allows(index.access, declaration.access)
                        || index
                            .index
                            .nominal_references()
                            .into_iter()
                            .any(|id| !self.types.iter().any(|ty| &ty.id == id))
                    {
                        return Err(HostInterfaceError::InvalidDeclaration);
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
                HostPathSegmentDeclaration::Virtual(virtual_step) => {
                    virtual_step.validate(definitions)?;
                    if !allows(virtual_step.access, declaration.access) {
                        return Err(HostInterfaceError::InvalidDeclaration);
                    }
                    HostPathSegmentContract {
                        input: HostPathInput::Virtual {
                            name: virtual_step.name.clone(),
                        },
                        result: virtual_step.result.clone(),
                        access: virtual_step.access,
                        member_fingerprint: 0,
                    }
                }
            };
            for nominal in resolved.result.nominal_references() {
                if !self.types.iter().any(|ty| &ty.id == nominal) {
                    return Err(HostInterfaceError::InvalidDeclaration);
                }
            }
            current = resolved.result.clone();
            segments.push(resolved);
        }
        if dynamic_parameters
            .keys()
            .copied()
            .enumerate()
            .any(|(expected, actual)| actual as usize != expected)
        {
            return Err(HostInterfaceError::InvalidDeclaration);
        }
        Ok(HostPathContract {
            root_fingerprint: root.fingerprint(definitions)?,
            result: current,
            schema_epoch: declaration.schema_epoch,
            access: declaration.access,

            segments,
        })
    }
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

fn read_only_collection(ty: &HostValueType<DefinitionId>) -> bool {
    matches!(
        ty,
        HostValueType::List(..)
            | HostValueType::Set(_, CollectionAccess::ReadOnly)
            | HostValueType::Map {
                access: CollectionAccess::ReadOnly,
                ..
            }
    )
}

#[cfg(test)]
mod tests {
    use crate::host_interface::test_support::{
        FingerprintFixture, InterfaceFixture, PathFixture, ValidateFixture, decode_fixture,
    };

    use kagari_common::identity::{
        DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity,
    };

    #[test]
    fn index_contract_cannot_upgrade_readonly_collection_access() {
        let mut index = super::HostIndexSegmentDeclaration {
            slot: 0,
            collection: super::HostValueType::List(
                DefinitionPath {
                    module: ModuleIdentity::single_file("collections.kgr"),
                    path: vec![DefinitionPathSegment {
                        kind: DefinitionKind::Trait,
                        name: "List".into(),
                        occurrence: 0,
                    }],
                },
                Box::new(super::HostValueType::I32),
            ),
            index: super::HostValueType::I32,
            result: super::HostValueType::I32,
            access: super::PathAccess::ReadWrite,
        };
        assert!(index.validate_fixture().is_err());
        index.access = super::PathAccess::ReadOnly;
        index.validate_fixture().unwrap();
        index.collection = super::HostValueType::Array(Box::new(super::HostValueType::I32));
        index.access = super::PathAccess::ReadWrite;
        index.validate_fixture().unwrap();
    }

    use super::*;
    use crate::host_interface::type_declaration::{HostFieldDeclaration, HostTypeDeclaration};

    #[test]
    fn mixed_path_roundtrips_offline_and_rejects_broken_segment_contracts() {
        let mut root = HostTypeDeclaration::new("game.Inventory");
        root.ownership = HostTypeOwnership::HostRoot;
        root.path_access = PathAccess::ReadWrite;
        let items_type = HostValueType::Array(Box::new(HostValueType::I32));
        let mut items = HostFieldDeclaration::new(&root.id, "items", items_type.clone());
        items.path_access = PathAccess::ReadWrite;
        items.writable = true;
        root.fields.push(items.clone());
        let declaration = HostPathDeclaration {
            root: root.id.clone(),
            segments: vec![
                HostPathSegmentDeclaration::Field(items.id),
                HostPathSegmentDeclaration::Index(HostIndexSegmentDeclaration {
                    slot: 0,
                    collection: items_type,
                    index: HostValueType::I32,
                    result: HostValueType::I32,
                    access: PathAccess::ReadOnly,
                }),
                HostPathSegmentDeclaration::Virtual(HostVirtualSegmentDeclaration {
                    name: "preview".into(),
                    result: HostValueType::I32,
                    access: PathAccess::ReadOnly,
                }),
            ],
            access: PathAccess::ReadOnly,
            schema_epoch: 4,
        };
        let interface = HostInterface {
            types: vec![root],
            functions: vec![],
            paths: vec![declaration.clone()],
        };
        let bytes = interface.encode_fixture().unwrap();
        let decoded = decode_fixture(&bytes).unwrap();
        assert_eq!(decoded, interface);
        assert_eq!(
            declaration
                .contract_fixture(&interface)
                .unwrap()
                .fingerprint_fixture()
                .unwrap(),
            decoded.paths[0]
                .contract_fixture(&decoded)
                .unwrap()
                .fingerprint_fixture()
                .unwrap()
        );
        let mut old = bytes.clone();
        old[4..6].copy_from_slice(&6u16.to_le_bytes());
        assert!(matches!(
            decode_fixture(&old),
            Err(HostInterfaceError::Version)
        ));

        let mut broken = interface.clone();
        if let HostPathSegmentDeclaration::Index(index) = &mut broken.paths[0].segments[1] {
            index.collection = HostValueType::I32;
        } else {
            panic!("expected index segment");
        }
        assert_eq!(
            broken.validate_fixture(),
            Err(HostInterfaceError::InvalidDeclaration)
        );
        if let HostPathSegmentDeclaration::Index(index) = &mut broken.paths[0].segments[1] {
            index.collection = HostValueType::Array(Box::new(HostValueType::I32));
            index.index = HostValueType::opaque("game.Missing");
        }
        assert_eq!(
            broken.validate_fixture(),
            Err(HostInterfaceError::InvalidDeclaration)
        );
        if let HostPathSegmentDeclaration::Index(index) = &mut broken.paths[0].segments[1] {
            index.index = HostValueType::I32;
            index.slot = 1;
        }
        assert_eq!(
            broken.validate_fixture(),
            Err(HostInterfaceError::InvalidDeclaration)
        );
    }

    #[test]
    fn nested_field_contracts_resolve_nominal_owners_without_runtime_registration() {
        let mut child = HostTypeDeclaration::new("game.Child");
        let mut count = HostFieldDeclaration::new(&child.id, "count", HostValueType::I32);
        count.path_access = PathAccess::ReadOnly;
        child.fields.push(count.clone());
        let mut root = HostTypeDeclaration::new("game.Root");
        root.ownership = HostTypeOwnership::HostRoot;
        root.path_access = PathAccess::ReadOnly;
        let mut nested =
            HostFieldDeclaration::new(&root.id, "child", HostValueType::Opaque(child.id.clone()));
        nested.path_access = PathAccess::ReadOnly;
        root.fields.push(nested.clone());
        let catalog = HostInterface {
            paths: vec![],
            types: vec![root.clone(), child],
            functions: vec![],
        };
        let fields = [nested.id.clone(), count.id.clone()];
        let declaration = HostPathDeclaration {
            root: root.id.clone(),
            segments: fields
                .iter()
                .cloned()
                .map(HostPathSegmentDeclaration::Field)
                .collect(),
            access: PathAccess::ReadOnly,
            schema_epoch: 7,
        };
        let contract = declaration.contract_fixture(&catalog).unwrap();
        let mut published = catalog.clone();
        let mut next_schema = declaration.clone();
        next_schema.schema_epoch = 8;
        published.paths = vec![declaration.clone(), next_schema];
        let encoded = published.encode_fixture().unwrap();
        published.paths.reverse();
        assert_eq!(encoded, published.encode_fixture().unwrap());
        let decoded_paths = decode_fixture(&encoded).unwrap();
        assert_eq!(decoded_paths.paths.len(), 2);
        assert!(
            decoded_paths
                .paths
                .iter()
                .all(|path| path.contract_fixture(&decoded_paths).is_ok())
        );
        published.paths.push(declaration.clone());
        assert_eq!(
            published.validate_fixture(),
            Err(HostInterfaceError::DuplicateDeclaration)
        );
        published.paths = vec![declaration.clone()];
        published.paths[0].segments =
            vec![HostPathSegmentDeclaration::Field(count.id.clone()); 257];
        assert_eq!(
            published.validate_fixture(),
            Err(HostInterfaceError::TooLarge)
        );
        assert_eq!(contract.result, HostValueType::I32);
        assert_eq!(contract.segments.len(), 2);
        let decoded = decode_fixture(&catalog.encode_fixture().unwrap()).unwrap();
        assert_eq!(
            contract.fingerprint_fixture().unwrap(),
            declaration
                .contract_fixture(&decoded)
                .unwrap()
                .fingerprint_fixture()
                .unwrap()
        );
        for invalid in [
            vec![],
            vec![count.id.clone()],
            vec![nested.id.clone(), nested.id],
            vec![fields[0].clone(), count.id.clone(), count.id],
        ] {
            assert!(
                catalog
                    .path_fixture(&HostPathDeclaration {
                        segments: invalid
                            .into_iter()
                            .map(HostPathSegmentDeclaration::Field)
                            .collect(),
                        ..declaration.clone()
                    })
                    .is_err()
            );
        }
    }
}

mod mapping;
