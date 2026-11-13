//! Checked external-ID views and capability weakening at native boundaries.
use crate::{
    error::RuntimeError,
    host::{HostBorrowKind, HostBorrowTable, HostRegistry, path_access_allows},
    native::binding::NativeResult,
    resource::ResourceState,
    value::{
        EphemeralValue,
        carriers::ScopedValue,
        host_values::{EphemeralObject, HostRootObject, paths::HostPathObject},
        kind::ObjectKind,
        properties::ValueProperties,
    },
};
use kagari_gc::{access::Scope, error::GcError, object::Gc};
use kagari_types::declaration::host::PathAccess;

impl HostRegistry {
    /// Validate a live external capability before creating an immutable shared
    /// alias. All original path fields remain edges in the new actual provider.
    pub(crate) fn narrow_argument_scoped<'gc>(
        &self,
        cx: &mut Scope<'gc>,
        value: ScopedValue<'gc>,
        borrows: &HostBorrowTable,
        resources: &ResourceState,
    ) -> Result<NativeResult<Option<ScopedValue<'gc>>>, GcError> {
        let Some(object) = value.object() else {
            return Ok(Err(invalid()));
        };
        match value.kind() {
            Some(ObjectKind::HostRoot) => {
                let root = cx.downcast::<HostRootObject>(object)?;
                if !self.matches_root(cx.read(root, HostRootObject::handle)) {
                    return Ok(Err(invalid()));
                }
                Ok(Ok(None))
            }
            Some(ObjectKind::HostPath) => {
                let path = cx.downcast::<HostPathObject>(object)?;
                if let Err(error) = self.validate_path_scoped(cx, path, resources)? {
                    return Ok(Err(error));
                }
                let mut facts = cx.read(path, HostPathObject::facts);
                if facts.access == PathAccess::ReadOnly {
                    return Ok(Ok(None));
                }
                facts.access = PathAccess::ReadOnly;
                let base = HostPathObject::base(cx, path);
                let count = cx.read(path, HostPathObject::len);
                let mut arguments = Vec::new();
                arguments.try_reserve_exact(count)?;
                for index in 0..count {
                    if index != 0
                        && index.is_multiple_of(1024)
                        && let Err(error) = resources.poll_closed()
                    {
                        return Ok(Err(error));
                    }
                    let ty = cx
                        .read(path, |path| path.argument_type(index))
                        .ok_or(GcError::Index)?;
                    arguments.push((ty, HostPathObject::argument(cx, path, index)?));
                }
                HostPathObject::alloc(cx, facts, base, &arguments)
                    .map(|path| Ok(Some(ScopedValue::managed(path))))
            }
            Some(ObjectKind::Ephemeral) => {
                let object = cx.downcast::<EphemeralObject>(object)?;
                let (EphemeralValue::HostRef(token) | EphemeralValue::HostMut(token)) =
                    cx.read(object, EphemeralObject::value)
                else {
                    return Ok(Err(invalid()));
                };
                if let Err(error) = borrows.validate(token, HostBorrowKind::Shared) {
                    return Ok(Err(error));
                }
                if token.borrow_kind() == HostBorrowKind::Shared {
                    return Ok(Ok(None));
                }
                EphemeralObject::alloc(cx, EphemeralValue::HostRef(token.shared()))
                    .map(|value| Ok(Some(value)))
            }
            _ => Ok(Err(invalid())),
        }
    }

    /// Validate the current registry/epoch, complete access chain and labeled
    /// dynamic fields while their actual provider remains admitted. No Host call.
    pub(crate) fn validate_path_scoped<'gc>(
        &self,
        cx: &Scope<'gc>,
        path: Gc<'gc, HostPathObject>,
        resources: &ResourceState,
    ) -> Result<NativeResult<()>, GcError> {
        let root = cx.read(path, |path| path.facts().root);
        if !self.matches_root(root) {
            return Ok(Err(invalid()));
        }
        let mut current = Some(path);
        let mut inspected = 0usize;
        while let Some(path) = current {
            inspected += 1;
            if inspected == 1024 {
                inspected = 0;
                if let Err(error) = resources.poll_closed() {
                    return Ok(Err(error));
                }
            }
            let facts = cx.read(path, HostPathObject::facts);
            let Some(descriptor) = self.path_descriptors.get(&facts.descriptor) else {
                return Ok(Err(invalid()));
            };
            let base = HostPathObject::base(cx, path);
            let input = base.map_or(root.type_id(), |base| {
                cx.read(base, |base| base.facts().result)
            });
            if facts.root != root
                || facts.epoch != root.schema_epoch()
                || descriptor.schema_epoch != facts.epoch
                || descriptor.root_type != input
                || descriptor.result_type != facts.result
                || !path_access_allows(descriptor.access, facts.access)
                || base.is_some_and(|base| {
                    !path_access_allows(cx.read(base, |base| base.facts().access), facts.access)
                })
                || cx.read(path, HostPathObject::len) != descriptor.dynamic_parameters.len()
            {
                return Ok(Err(invalid()));
            }
            for (index, parameter) in descriptor.dynamic_parameters.iter().enumerate() {
                if index != 0
                    && index.is_multiple_of(1024)
                    && let Err(error) = resources.poll_closed()
                {
                    return Ok(Err(error));
                }
                if cx.read(path, |path| path.argument_type(index)) != Some(parameter.ty)
                    || !ValueProperties::of(cx, HostPathObject::argument(cx, path, index)?)?
                        .storable
                {
                    return Ok(Err(invalid()));
                }
            }
            current = base;
        }
        Ok(Ok(()))
    }
}

fn invalid() -> RuntimeError {
    RuntimeError::typed_path_validation("invalid external path view or borrow capability")
}

#[cfg(test)]
mod tests {
    use crate::{
        Runtime, RuntimeConfig,
        error::RuntimeErrorKind,
        execution::{memory::GcHeapConfig, roots::ExecutionRoots},
        host::{HostBorrowKind, HostObjectId},
        metadata::TypeId,
        value::{EphemeralValue, host_values::EphemeralObject, view::ValueView},
    };

    #[test]
    fn shared_external_alias_preserves_owner_epoch_and_original_unique_token() {
        let mut runtime = Runtime::new(RuntimeConfig {
            gc: GcHeapConfig {
                collection_threshold: Some(1),
                ..Default::default()
            },
            ..Default::default()
        });
        let baseline = runtime.gc_stats();
        let mut frame = runtime.host_scope(&[]).unwrap();
        let ty = TypeId::new(0);
        let original = frame.borrows().borrow_unique(HostObjectId(7), ty).unwrap();
        let rooted = {
            let Runtime {
                memory, resources, ..
            } = frame.runtime_mut();
            memory.scope(resources, |cx, _| {
                EphemeralObject::alloc(cx, EphemeralValue::HostMut(original))?.retain(cx)
            })
        }
        .unwrap();
        let value = frame.import(&rooted).unwrap();
        drop(rooted);
        let shared = {
            let Runtime {
                memory,
                host,
                host_borrows,
                resources,
                ..
            } = frame.runtime_mut();
            memory
                .scope(resources, |cx, roots| {
                    ExecutionRoots::with(roots, cx, |slots| {
                        let original = value.get(slots)?;
                        let shared = host
                            .narrow_argument_scoped(
                                slots.scope(),
                                original,
                                host_borrows,
                                resources,
                            )?
                            .unwrap()
                            .expect("changed capability");
                        // Source publication, rather than a retained Host root,
                        // keeps the actual alias provider alive across collection.
                        let location =
                            slots.push_local(value.location().unwrap().execution, shared)?;
                        ValueView::protected(shared, Some(location))
                    })
                })
                .unwrap()
        };
        frame.runtime_mut().collect_garbage().unwrap();
        assert_eq!(
            frame.runtime().gc_stats().root_registrations,
            baseline.root_registrations
        );
        let (shared, retained_shared) = {
            let Runtime {
                memory,
                host,
                host_borrows,
                resources,
                ..
            } = frame.runtime_mut();
            memory
                .scope(resources, |cx, roots| {
                    ExecutionRoots::with(roots, cx, |slots| {
                        let original_value = value.get(slots)?.object().unwrap();
                        let original_value =
                            slots.scope().downcast::<EphemeralObject>(original_value)?;
                        assert_eq!(
                            slots.scope().read(original_value, EphemeralObject::value),
                            EphemeralValue::HostMut(original)
                        );
                        let shared_value = shared.get(slots)?;
                        assert!(
                            host.narrow_argument_scoped(
                                slots.scope(),
                                shared_value,
                                host_borrows,
                                resources
                            )?
                            .unwrap()
                            .is_none()
                        );
                        let object = slots
                            .scope()
                            .downcast::<EphemeralObject>(shared_value.object().unwrap())?;
                        Ok((
                            slots.scope().read(object, EphemeralObject::value),
                            shared_value.retain(slots.scope())?,
                        ))
                    })
                })
                .unwrap()
        };
        let EphemeralValue::HostRef(shared) = shared else {
            panic!("shared token")
        };
        assert_eq!(shared, original.shared());
        frame
            .runtime()
            .validate_host_borrow(shared, HostBorrowKind::Shared)
            .unwrap();
        assert_eq!(
            frame
                .runtime()
                .validate_host_borrow(shared, HostBorrowKind::Unique)
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::HostBorrowConflict
        );
        frame
            .runtime()
            .validate_host_borrow(original, HostBorrowKind::Unique)
            .unwrap();
        let other = Runtime::default();
        assert_eq!(
            other
                .validate_host_borrow(shared, HostBorrowKind::Shared)
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::ExpiredHostBorrow
        );
        drop(frame);
        assert_eq!(
            runtime
                .validate_host_borrow(shared, HostBorrowKind::Shared)
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::ExpiredHostBorrow
        );
        assert_eq!(
            runtime
                .validate_host_borrow(original, HostBorrowKind::Unique)
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::ExpiredHostBorrow
        );
        // Retaining the actual wrapper cannot extend the Host reservation.
        let expired = {
            let Runtime {
                memory,
                host,
                host_borrows,
                resources,
                ..
            } = &mut runtime;
            memory
                .scope(resources, |cx, _| {
                    let value = retained_shared.get(cx)?;
                    Ok(host
                        .narrow_argument_scoped(cx, value, host_borrows, resources)?
                        .map(|alias| alias.is_some()))
                })
                .unwrap()
                .unwrap_err()
        };
        assert_eq!(expired.kind(), RuntimeErrorKind::ExpiredHostBorrow);
        drop(retained_shared);
        runtime.collect_garbage().unwrap();
        assert_eq!(runtime.gc_stats().objects, baseline.objects);
        assert_eq!(
            runtime.gc_stats().root_registrations,
            baseline.root_registrations
        );
    }
}
