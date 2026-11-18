//! Checked Host paths and shared aliases at native boundaries.
use crate::{
    error::RuntimeError,
    host::{HostRegistry, path_access_allows},
    native::binding::NativeResult,
    resource::ResourceState,
    value::{
        carriers::ScopedValue,
        host_values::{HostRootObject, paths::HostPathObject},
        kind::ObjectKind,
        properties::ValueProperties,
    },
};
use kagari_gc::{access::Scope, error::GcError, object::Gc};
use kagari_types::declaration::host::PathAccess;

impl HostRegistry {
    /// Validate a live Host path before creating an immutable shared
    /// alias. All original path fields remain edges in the new actual provider.
    pub(crate) fn narrow_argument_scoped<'gc>(
        &self,
        cx: &mut Scope<'gc>,
        value: ScopedValue<'gc>,
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
    RuntimeError::typed_path_validation("invalid Host path view")
}
