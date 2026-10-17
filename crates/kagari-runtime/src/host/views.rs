//! Checked external-ID views and capability weakening at native boundaries.
use crate::{
    Runtime,
    error::RuntimeError,
    gc::GcHeap,
    host::{
        HostBorrowKind, HostPathViewHandle, HostRegistry, path_access_allows,
        validate_dynamic_arguments,
    },
    metadata::TypeId,
    native::binding::NativeResult,
    value::{EphemeralValue, Value},
};
use kagari_types::declaration::host::PathAccess;
use std::iter;

impl HostRegistry {
    pub(crate) fn validate_view(
        &self,
        heap: &GcHeap,
        view: &HostPathViewHandle,
    ) -> NativeResult<()> {
        if !self.matches_root(view.root()) {
            return Err(invalid());
        }
        for current in iter::successors(Some(view), |view| view.base()) {
            let descriptor = self
                .path_descriptors
                .get(&current.descriptor_id())
                .ok_or_else(invalid)?;
            let input = current
                .base()
                .map_or(current.root().type_id(), HostPathViewHandle::result_type);
            if current.root() != view.root()
                || current.schema_epoch() != current.root().schema_epoch()
                || descriptor.schema_epoch != current.schema_epoch()
                || descriptor.root_type != input
                || descriptor.result_type != current.result_type()
                || !path_access_allows(descriptor.access, current.access())
                || current
                    .base()
                    .is_some_and(|base| !path_access_allows(base.access(), current.access()))
            {
                return Err(invalid());
            }
            validate_dynamic_arguments(heap, descriptor, current.dynamic_args())?;
        }
        Ok(())
    }
}

impl Runtime {
    pub(crate) fn matches_host_identity(&self, value: &Value, expected: TypeId) -> bool {
        if !self.gc.validate_value(value) {
            return false;
        }
        match value {
            Value::HostRoot(id) => self
                .gc
                .host_root(*id)
                .is_some_and(|root| root.type_id() == expected && self.host.matches_root(root)),
            Value::HostPathView(id) => self.gc.host_path(*id).is_some_and(|view| {
                view.result_type() == expected && self.host.validate_view(&self.gc, &view).is_ok()
            }),
            Value::Ephemeral(id) => match self.gc.ephemeral(*id) {
                Some(EphemeralValue::HostRef(token)) => {
                    token.type_id() == expected
                        && self
                            .validate_host_borrow(token, HostBorrowKind::Shared)
                            .is_ok()
                }
                Some(EphemeralValue::HostMut(token)) => {
                    token.type_id() == expected
                        && self
                            .validate_host_borrow(token, HostBorrowKind::Unique)
                            .is_ok()
                }
                _ => false,
            },
            _ => false,
        }
    }

    pub(crate) fn narrow_host_argument(&self, value: &Value) -> NativeResult<Option<Value>> {
        match value {
            Value::HostRoot(_) => Ok(None),
            Value::HostPathView(id) => {
                let view = self.gc.host_path(*id).ok_or_else(invalid)?;
                self.host.validate_view(&self.gc, &view)?;
                if view.access() == PathAccess::ReadOnly {
                    return Ok(None);
                }
                let mut shared = view.as_ref().clone();
                shared.access = PathAccess::ReadOnly;
                self.gc.alloc_host_path(shared).map(Some)
            }
            Value::Ephemeral(id) => {
                let Some(EphemeralValue::HostRef(token) | EphemeralValue::HostMut(token)) =
                    self.gc.ephemeral(*id)
                else {
                    return Err(invalid());
                };
                self.validate_host_borrow(token, HostBorrowKind::Shared)?;
                if token.borrow_kind() == HostBorrowKind::Shared {
                    return Ok(None);
                }
                self.gc
                    .alloc_ephemeral(EphemeralValue::HostRef(token.shared()))
                    .map(Some)
            }
            _ => Err(invalid()),
        }
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
        gc::GcHeapConfig,
        host::{HostBorrowKind, HostObjectId},
        metadata::TypeId,
        value::{EphemeralValue, Value},
    };

    #[test]
    fn shared_external_alias_preserves_owner_epoch_and_original_unique_token() {
        let runtime = Runtime::new(RuntimeConfig {
            gc: GcHeapConfig {
                collection_threshold: Some(1),
            },
            ..Default::default()
        });
        let frame = runtime.host_scope(&[]).unwrap();
        let ty = TypeId::new(0);
        let original = frame.borrows().borrow_unique(HostObjectId(7), ty).unwrap();
        let value = runtime.gc().alloc_host_mut(original).unwrap();
        let roots = runtime
            .gc()
            .root_execution_values(vec![value, Value::Unit])
            .unwrap();
        let shared = runtime.narrow_host_argument(&value).unwrap().unwrap();
        roots.set(runtime.gc(), 1, shared).unwrap();
        runtime.collect_garbage().unwrap();
        let Value::Ephemeral(id) = roots.get(runtime.gc(), 1).unwrap() else {
            panic!("alias")
        };
        let Some(EphemeralValue::HostRef(shared)) = runtime.gc().ephemeral(id) else {
            panic!("shared token")
        };
        runtime
            .validate_host_borrow(shared, HostBorrowKind::Shared)
            .unwrap();
        assert_eq!(
            runtime
                .validate_host_borrow(shared, HostBorrowKind::Unique)
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::HostBorrowConflict
        );
        runtime
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
    }
}
