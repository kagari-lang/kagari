use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
};

use crate::{error::RuntimeError, reload::ModuleEpoch, security::CapabilitySet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeId(u64);

impl TypeId {
    pub fn new(index: usize) -> Self {
        Self(index as u64)
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FieldMetadataId(u64);

impl FieldMetadataId {
    pub fn new(index: usize) -> Self {
        Self(index as u64)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VariantMetadataId(u64);

impl VariantMetadataId {
    pub fn new(index: usize) -> Self {
        Self(index as u64)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MethodMetadataId(u64);

impl MethodMetadataId {
    pub fn new(index: usize) -> Self {
        Self(index as u64)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct AbiFingerprint(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeKind {
    Primitive,
    Tuple,
    Array,
    Map,
    Struct,
    Enum,
    Function,
    Interface,
    DynamicInterfaceObject,
    HostObject,
    HostPathView,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Private,
    Public,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathAccess {
    None,
    ReadOnly,
    ReadWrite,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldInfo {
    pub id: FieldMetadataId,
    pub name: String,
    pub ty: TypeId,
    pub readable: bool,
    pub writable: bool,
    pub visibility: Visibility,
    pub path_access: PathAccess,
    pub abi_fingerprint: AbiFingerprint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantInfo {
    pub id: VariantMetadataId,
    pub name: String,
    pub payload: Vec<TypeId>,
    pub abi_fingerprint: AbiFingerprint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParameterInfo {
    pub name: String,
    pub ty: TypeId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MethodOrigin {
    Inherent,
    Trait(TypeId),
    Host,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodInfo {
    pub id: MethodMetadataId,
    pub name: String,
    pub params: Vec<ParameterInfo>,
    pub return_type: TypeId,
    pub origin: MethodOrigin,
    pub capability_requirements: CapabilitySet,
    pub abi_fingerprint: AbiFingerprint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraitInfo {
    pub trait_type: TypeId,
    pub name: String,
    pub abi_fingerprint: AbiFingerprint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeInfo {
    pub id: TypeId,
    pub name: String,
    pub kind: TypeKind,
    pub epoch: Option<ModuleEpoch>,
    pub fields: Vec<FieldInfo>,
    pub variants: Vec<VariantInfo>,
    pub methods: Vec<MethodInfo>,
    pub traits: Vec<TraitInfo>,
    pub abi_fingerprint: AbiFingerprint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeRegistration {
    pub name: String,
    pub kind: TypeKind,
    pub epoch: Option<ModuleEpoch>,
    pub fields: Vec<FieldInfo>,
    pub variants: Vec<VariantInfo>,
    pub methods: Vec<MethodInfo>,
    pub traits: Vec<TraitInfo>,
    pub abi_fingerprint: AbiFingerprint,
}

impl TypeRegistration {
    pub fn new(name: impl Into<String>, kind: TypeKind) -> Self {
        Self {
            name: name.into(),
            kind,
            epoch: None,
            fields: Vec::new(),
            variants: Vec::new(),
            methods: Vec::new(),
            traits: Vec::new(),
            abi_fingerprint: AbiFingerprint::default(),
        }
    }
}

#[derive(Debug, Default)]
pub struct TypeRegistry {
    inner: RefCell<TypeRegistryInner>,
}

#[derive(Debug, Default)]
struct TypeRegistryInner {
    by_id: Vec<TypeInfo>,
    by_name: HashMap<String, TypeId>,
    public_abi_fingerprints: HashSet<AbiFingerprint>,
}

impl TypeRegistry {
    pub fn register(&self, registration: TypeRegistration) -> Result<TypeId, RuntimeError> {
        let mut inner = self.inner.borrow_mut();
        if inner.by_name.contains_key(&registration.name) {
            return Err(RuntimeError::metadata_conflict(registration.name));
        }

        let id = TypeId::new(inner.by_id.len());
        let info = TypeInfo {
            id,
            name: registration.name.clone(),
            kind: registration.kind,
            epoch: registration.epoch,
            fields: registration.fields,
            variants: registration.variants,
            methods: registration.methods,
            traits: registration.traits,
            abi_fingerprint: registration.abi_fingerprint,
        };

        inner
            .public_abi_fingerprints
            .insert(registration.abi_fingerprint);
        inner.by_name.insert(registration.name, id);
        inner.by_id.push(info);
        Ok(id)
    }

    pub fn get(&self, id: TypeId) -> Option<TypeInfo> {
        self.inner.borrow().by_id.get(id.index()).cloned()
    }

    pub fn get_by_name(&self, name: &str) -> Option<TypeInfo> {
        let inner = self.inner.borrow();
        let id = inner.by_name.get(name)?;
        inner.by_id.get(id.index()).cloned()
    }

    pub fn id_by_name(&self, name: &str) -> Option<TypeId> {
        self.inner.borrow().by_name.get(name).copied()
    }

    pub fn len(&self) -> usize {
        self.inner.borrow().by_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn public_abi_fingerprints(&self) -> Vec<AbiFingerprint> {
        self.inner
            .borrow()
            .public_abi_fingerprints
            .iter()
            .copied()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::RuntimeErrorKind;

    #[test]
    fn registers_type_metadata_by_id_and_name() {
        let registry = TypeRegistry::default();
        let i32_id = registry
            .register(TypeRegistration {
                abi_fingerprint: AbiFingerprint(11),
                ..TypeRegistration::new("i32", TypeKind::Primitive)
            })
            .unwrap();
        let player_id = registry
            .register(TypeRegistration {
                fields: vec![FieldInfo {
                    id: FieldMetadataId::new(0),
                    name: "health".to_owned(),
                    ty: i32_id,
                    readable: true,
                    writable: true,
                    visibility: Visibility::Public,
                    path_access: PathAccess::ReadWrite,
                    abi_fingerprint: AbiFingerprint(12),
                }],
                methods: vec![MethodInfo {
                    id: MethodMetadataId::new(0),
                    name: "heal".to_owned(),
                    params: vec![ParameterInfo {
                        name: "amount".to_owned(),
                        ty: i32_id,
                    }],
                    return_type: i32_id,
                    origin: MethodOrigin::Inherent,
                    capability_requirements: CapabilitySet::default(),
                    abi_fingerprint: AbiFingerprint(13),
                }],
                abi_fingerprint: AbiFingerprint(14),
                ..TypeRegistration::new("Player", TypeKind::Struct)
            })
            .unwrap();

        assert_eq!(registry.id_by_name("Player"), Some(player_id));
        let player = registry.get_by_name("Player").unwrap();
        assert_eq!(player.id, player_id);
        assert_eq!(player.fields[0].name, "health");
        assert_eq!(player.methods[0].name, "heal");
        assert!(
            registry
                .public_abi_fingerprints()
                .contains(&AbiFingerprint(14))
        );
    }

    #[test]
    fn rejects_duplicate_type_names() {
        let registry = TypeRegistry::default();
        registry
            .register(TypeRegistration::new("Player", TypeKind::Struct))
            .unwrap();
        let error = registry
            .register(TypeRegistration::new("Player", TypeKind::Struct))
            .unwrap_err();

        assert_eq!(error.kind(), RuntimeErrorKind::MetadataConflict);
    }
}
