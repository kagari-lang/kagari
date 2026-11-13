//! Closed field offsets retain immutable layout provenance, never executable roots.
use crate::{
    error::RuntimeError,
    frame::types::compatibility::TypeIdentity,
    host::HostRegistryId,
    module::{InstallationId, ModuleKey, StructLayoutRef, layout_identity::LayoutIdentity},
    value::scalar::fields::ScalarEncoding,
};
use kagari_bytecode::instruction::StructId;
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::{
    collections::HashMap,
    sync::{Arc, Weak},
};

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
        index: usize,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct StructStorageLayout {
    pub(crate) semantic: StructLayoutRef,
    pub(crate) fields: Box<[StoredField]>,
    pub(crate) scalar_bytes: usize,
    pub(crate) managed_count: usize,
}

impl StructStorageLayout {
    pub(crate) fn prepare(semantic: StructLayoutRef) -> Option<Self> {
        let mut fields = Vec::with_capacity(semantic.layout().fields.len());
        let mut managed_count = 0_usize;
        let mut size = 0_usize;
        for field in &semantic.layout().fields {
            if !field.ty.is_concrete() {
                return None;
            }
            let (encoding, width) = match &field.ty {
                Ty::Builtin(ty) if *ty != BuiltinType::String => {
                    let encoding = ScalarEncoding::new(*ty)?;
                    (
                        StoredField::Scalar {
                            offset: size,
                            encoding,
                        },
                        encoding.width,
                    )
                }
                _ => {
                    let index = managed_count;
                    managed_count = managed_count.checked_add(1)?;
                    (StoredField::Managed { index }, 0)
                }
            };
            size = size.checked_add(width)?;
            fields.push(encoding);
        }
        Some(Self {
            semantic,
            fields: fields.into_boxed_slice(),
            scalar_bytes: size,
            managed_count,
        })
    }
}

/// Weak immutable descriptions are shared by interpreter and Host construction.
/// Cache keys/entries never grant object or executable liveness; GC prunes dead
/// descriptions after provider retirement, without per-allocation table scans.
#[derive(Debug, Default)]
pub(crate) struct StructStorageLayouts {
    entries: HashMap<StructStorageKey, Weak<StructStorageLayout>>,
}

impl StructStorageLayouts {
    pub(crate) fn prepare(
        &mut self,
        layout: StructLayoutRef,
    ) -> Result<Arc<StructStorageLayout>, RuntimeError> {
        let key = layout.storage_key();
        if let Some(existing) = self.entries.get(&key).and_then(Weak::upgrade) {
            return Ok(existing);
        }
        let description = Arc::new(
            StructStorageLayout::prepare(layout)
                .ok_or_else(|| RuntimeError::module_validation("closed struct storage layout"))?,
        );
        self.entries
            .try_reserve(1)
            .map_err(|_| RuntimeError::resource_limit("struct description registry"))?;
        self.entries.insert(key, Arc::downgrade(&description));
        Ok(description)
    }

    pub(crate) fn prune(&mut self) {
        self.entries
            .retain(|_, description| description.strong_count() != 0);
    }
}
