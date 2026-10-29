//! Executable identity leases outlive disposable physical struct descriptions.
use crate::{
    error::RuntimeError,
    gc::{
        directory::{
            PayloadKind,
            descriptors::{CacheKey, Descriptor, DescriptorId, Descriptors, Entry},
        },
        object_header::ObjectHeader,
        weak_cache::WeakCacheToken,
    },
    module::{
        StructLayoutRef,
        layout_storage::{StructStorageKey, StructStorageLayout},
    },
    native::binding::NativeResult,
};
use std::{
    alloc::Layout,
    sync::{Arc, OnceLock},
};

/// Shared exact identity and bounds, without field or reference scanning tables.
/// The descriptor directory cannot recycle this ID while any preparation or
/// physical descriptor retains the lease. It never retains script objects.
#[derive(Debug)]
pub(in crate::gc) struct StructIdentity {
    id: OnceLock<DescriptorId>,
    pub(in crate::gc) semantic: StructLayoutRef,
    pub(in crate::gc) size: usize,
}

impl StructIdentity {
    pub(in crate::gc) fn registered_id(&self) -> u32 {
        self.id.get().expect("registered struct identity").get()
    }
}

impl Descriptor {
    pub(in crate::gc) fn structure(description: StructStorageLayout) -> NativeResult<Arc<Self>> {
        let identity = Arc::new(StructIdentity {
            id: OnceLock::new(),
            semantic: description.semantic.clone(),
            size: description.size,
        });
        Self::with_structure(description, identity)
    }

    fn with_structure(
        description: StructStorageLayout,
        identity: Arc<StructIdentity>,
    ) -> NativeResult<Arc<Self>> {
        let (layout, offset) = Layout::from_size_align(description.size, 1)
            .ok()
            .and_then(ObjectHeader::layout)
            .ok_or_else(|| RuntimeError::resource_limit("struct inline layout"))?;
        let mut descriptor = Self::new(
            PayloadKind::Struct,
            layout,
            offset,
            !description.references.is_empty(),
            |_| {},
        );
        descriptor.identity = Some(identity);
        descriptor.structure = Some(Arc::new(description));
        // Packed fields have no Rust-owned values to destroy.
        descriptor.destroy = None;
        Ok(Arc::new(descriptor))
    }

    pub(in crate::gc) fn struct_identity(&self) -> &Arc<StructIdentity> {
        self.identity.as_ref().expect("struct descriptor identity")
    }
}

impl Descriptors {
    #[cfg(feature = "execution-diagnostics")]
    pub(in crate::gc::directory) fn struct_identity_usage(&self) -> (usize, usize) {
        let count = self
            .entries
            .iter()
            .filter(|entry| {
                matches!(
                    entry,
                    Entry::Admitted {
                        cache: CacheKey::Structure(_),
                        ..
                    } | Entry::Idle {
                        cache: CacheKey::Structure(_),
                        ..
                    }
                )
            })
            .count();
        (
            count * (size_of::<StructIdentity>() + 2 * size_of::<usize>()),
            count,
        )
    }

    pub(in crate::gc::directory) fn register_structure(
        &mut self,
        descriptor: &Arc<Descriptor>,
        token: WeakCacheToken<StructStorageKey, StructIdentity>,
    ) -> NativeResult<()> {
        let Some(identity) = &descriptor.identity else {
            return Err(RuntimeError::module_validation("invalid struct descriptor"));
        };
        if !token.is_for(identity) || identity.id.get().is_some() {
            return Err(RuntimeError::module_validation(
                "invalid struct cache admission",
            ));
        }
        let id = self.register(descriptor, CacheKey::Structure(token))?;
        // Exclusive runtime admission: registration has no callback/reentry point.
        identity.id.set(id).expect("fresh struct identity");
        Ok(())
    }

    /// Reuse the physical description while present; otherwise reconstruct it
    /// under the same leased ID. No second interning map or permanent ID history.
    pub(in crate::gc::directory) fn struct_description(
        &mut self,
        identity: Arc<StructIdentity>,
    ) -> NativeResult<Arc<Descriptor>> {
        let id = *identity
            .id
            .get()
            .ok_or_else(|| RuntimeError::module_validation("unregistered struct identity"))?;
        let entry = self
            .entries
            .get_mut(id.index())
            .ok_or_else(|| RuntimeError::module_validation("foreign struct identity"))?;
        match entry {
            Entry::Admitted { descriptor, .. }
                if descriptor
                    .identity
                    .as_ref()
                    .is_some_and(|current| Arc::ptr_eq(current, &identity)) =>
            {
                return Ok(descriptor.clone());
            }
            Entry::Idle {
                descriptor,
                cache: CacheKey::Structure(token),
            } if token.is_for(&identity) => {
                if let Some(current) = descriptor.as_ref().and_then(|value| value.upgrade()) {
                    return Ok(current);
                }
                let layout =
                    StructStorageLayout::prepare(identity.semantic.clone()).ok_or_else(|| {
                        RuntimeError::module_validation("closed struct storage layout")
                    })?;
                let current = Descriptor::with_structure(layout, identity)?;
                current.id.set(id).expect("fresh physical descriptor");
                *descriptor = Some(Arc::downgrade(&current));
                return Ok(current);
            }
            _ => {}
        }
        Err(RuntimeError::module_validation("foreign struct identity"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Runtime,
        gc::weak_cache::{WEAK_CACHE_REMOVE_WORK, WeakCache},
        native::storage::scan::{ScanBudget, ScanStatus},
        test_support::layouts::layout,
    };
    use kagari_types::{scalar::BuiltinType, ty::Ty};

    #[test]
    fn identity_lease_rebuilds_storage_and_delays_id_reuse_until_last_owner() {
        let mut runtime = Runtime::default();
        let semantic = layout(
            &mut runtime,
            "Leased",
            &[("x", Ty::Builtin(BuiltinType::I32), true)],
        );
        let original =
            Descriptor::structure(StructStorageLayout::prepare(semantic.clone()).unwrap()).unwrap();
        let identity = original.struct_identity().clone();
        let weak_original = Arc::downgrade(&original);
        let mut cache = WeakCache::default();
        let insertion = cache
            .prepare_insert(semantic.storage_key(), &identity)
            .unwrap();
        let mut owner = Descriptors::default();
        owner
            .register_structure(&original, insertion.token())
            .unwrap();
        insertion.commit();
        let id = owner.admit(original.clone()).unwrap();
        owner.release(id);
        drop(original);
        assert!(weak_original.upgrade().is_none());
        drop(weak_original);

        let mut cursor = owner.begin_maintenance();
        let step = owner.maintenance_slice(&mut cursor, &mut ScanBudget::new(4), None);
        assert_eq!(step.status, ScanStatus::Complete);
        assert!(step.removed_structure.is_none());
        let unrelated = owner.builtin::<u64>(PayloadKind::Range, false).unwrap();
        assert_ne!(
            unrelated.id.get(),
            Some(&id),
            "a prepared field still owns this ID"
        );

        // Coincident numeric IDs in another runtime cannot authorize a rebuild.
        let mut foreign = Descriptors::default();
        let _foreign = foreign.builtin::<u64>(PayloadKind::Range, false).unwrap();
        assert!(foreign.struct_description(identity.clone()).is_err());

        let rebuilt = owner.struct_description(identity.clone()).unwrap();
        assert_eq!(owner.admit(rebuilt.clone()).unwrap(), id);
        assert!(Arc::ptr_eq(
            &rebuilt,
            &owner.struct_description(identity.clone()).unwrap()
        ));
        assert_eq!(rebuilt.structure_layout().fields.len(), 1);
        owner.release(id);
        drop(rebuilt);
        drop(identity);
        let mut cursor = owner.begin_maintenance();
        let step = owner.maintenance_slice(&mut cursor, &mut ScanBudget::new(4), None);
        let token = step.removed_structure.unwrap();
        let (status, detached) = cache.remove(&token, &mut ScanBudget::new(WEAK_CACHE_REMOVE_WORK));
        assert_eq!(status, ScanStatus::Complete);
        drop(detached);
        drop(token);
        assert!(cache.is_empty());
        let recycled = owner.builtin::<u32>(PayloadKind::HostRoot, false).unwrap();
        assert_eq!(
            recycled.id.get(),
            Some(&id),
            "identity retirement returns the ID to its owner"
        );
    }
}
