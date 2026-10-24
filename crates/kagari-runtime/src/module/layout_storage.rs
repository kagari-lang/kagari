//! Closed field offsets retain immutable layout provenance, never executable roots.
use crate::{
    frame::types::compatibility::TypeIdentity,
    host::HostRegistryId,
    module::{InstallationId, ModuleKey, StructLayoutRef, layout_identity::LayoutIdentity},
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::instruction::StructId;
use kagari_contract::representation::builtin_representation;
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct StructStorageKey {
    registry: HostRegistryId,
    installation: InstallationId,
    owner: ModuleKey,
    identity: StorageIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum StorageIdentity {
    // Canonical identities already include shape and nominal argument provenance.
    // Their counters are never reused within the supplying program generation.
    Canonical(LayoutIdentity),
    Detached {
        structure: StructId,
        arguments: Vec<Ty>,
        provenance: Option<Arc<[Arc<TypeIdentity>]>>,
    },
}

impl StructLayoutRef {
    pub(crate) fn storage_key(&self) -> StructStorageKey {
        StructStorageKey {
            registry: self.module.registry_owner,
            installation: self.module.installation_id(),
            owner: self.module.key(),
            identity: match self.canonical {
                Some(identity) => StorageIdentity::Canonical(identity),
                None => StorageIdentity::Detached {
                    structure: self.id,
                    arguments: self.layout().arguments.clone(),
                    provenance: self.scope.as_ref().map(|scope| scope.arguments().clone()),
                },
            },
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum StoredField {
    Scalar {
        offset: usize,
        encoding: ScalarEncoding,
    },
    Managed {
        offset: usize,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct StructStorageLayout {
    pub(crate) semantic: StructLayoutRef,
    pub(crate) fields: Box<[StoredField]>,
    pub(crate) references: Box<[usize]>,
    pub(crate) size: usize,
}

impl StructStorageLayout {
    pub(crate) fn prepare(semantic: StructLayoutRef) -> Option<Self> {
        let mut fields = Vec::with_capacity(semantic.layout().fields.len());
        let mut references = Vec::new();
        let mut size = 0_usize;
        for (slot, field) in semantic.layout().fields.iter().enumerate() {
            if !field.ty.is_concrete() {
                return None;
            }
            let (encoding, width) = match &field.ty {
                Ty::Builtin(ty) if *ty != BuiltinType::String => (
                    StoredField::Scalar {
                        offset: size,
                        encoding: ScalarEncoding::new(*ty)?,
                    },
                    scalar_width(*ty)?,
                ),
                _ => {
                    references.push(slot);
                    // Representation discriminator plus checked owner/slot/generation.
                    (StoredField::Managed { offset: size }, 13)
                }
            };
            size = size.checked_add(width)?;
            fields.push(encoding);
        }
        Some(Self {
            semantic,
            fields: fields.into_boxed_slice(),
            references: references.into_boxed_slice(),
            size,
        })
    }
}

fn scalar_width(ty: BuiltinType) -> Option<usize> {
    Some(match ty {
        BuiltinType::Unit => 0,
        BuiltinType::Bool => 1,
        BuiltinType::Char | BuiltinType::F32 => 4,
        BuiltinType::F64 => 8,
        _ => (ty.integer_layout()?.0 / 8) as usize,
    })
}

/// Width/sign are derived once from the verified semantic scalar domain.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ScalarEncoding {
    pub(crate) ty: BuiltinType,
    pub(crate) representation: ValueType,
    pub(crate) width: usize,
    pub(crate) signed: bool,
}

impl ScalarEncoding {
    fn new(ty: BuiltinType) -> Option<Self> {
        Some(Self {
            ty,
            representation: builtin_representation(ty),
            width: scalar_width(ty)?,
            signed: ty.integer_layout().is_some_and(|(_, signed)| signed),
        })
    }
}
