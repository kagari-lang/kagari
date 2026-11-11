//! Typed Host path callbacks lend exclusive Runtime services and pin only their adapter.
use crate::{
    Runtime,
    error::RuntimeError,
    host::{
        HostBorrowTable, HostCallContext, HostPathAdapter, HostPathContext, HostPathDescriptorId,
        HostPathMutationRecord, HostPathOperation, apply_path_modify, path_scope_error,
    },
    value::Value,
};
use kagari_bytecode::instruction::BinaryOp;
use std::iter;

impl Runtime {
    pub(crate) fn read_path(
        &mut self,
        root_or_view: &Value,
        descriptor_id: HostPathDescriptorId,
        dynamic_args: Vec<Value>,
    ) -> Result<Value, RuntimeError> {
        self.gc.ensure_execution_allowed()?;
        let mut scope = self.path_roots(root_or_view, &dynamic_args, None)?;
        let context = scope.runtime().host.resolve_path_context(
            scope.runtime().gc(),
            root_or_view,
            descriptor_id,
            dynamic_args,
            HostPathOperation::Read,
        )?;
        scope
            .borrows()
            .borrow_shared(context.root.object_id(), context.root.type_id())?;
        let adapter = scope.runtime().host.path_adapter(context.descriptor.id)?;
        validate_path_operation(
            &adapter,
            &mut scope,
            &context,
            HostPathOperation::Read,
            None,
        )?;
        let read = adapter.read.as_ref().ok_or_else(|| {
            RuntimeError::typed_path_validation("path descriptor does not support reads")
        })?;
        let value = read(&mut scope, &context).map_err(|error| {
            RuntimeError::typed_path_validation(format!("host path read failed: {}", error))
        })?;
        HostBorrowTable::validate_no_escape(scope.runtime().gc(), &value)?;
        if !scope.runtime().gc().validate_value(&value) {
            return Err(RuntimeError::typed_path_validation(
                "invalid heap reference in path result",
            ));
        }
        Ok(value)
    }

    pub(crate) fn set_path(
        &mut self,
        root_or_view: &Value,
        descriptor_id: HostPathDescriptorId,
        dynamic_args: Vec<Value>,
        value: Value,
    ) -> Result<(), RuntimeError> {
        self.gc.ensure_execution_allowed()?;
        let mut scope = self.path_roots(root_or_view, &dynamic_args, Some(&value))?;
        let context = scope.runtime().host.resolve_path_context(
            scope.runtime().gc(),
            root_or_view,
            descriptor_id,
            dynamic_args,
            HostPathOperation::Set,
        )?;
        if !value.is_default_heap_payload(scope.runtime().gc()) {
            return Err(RuntimeError::typed_path_validation(
                "path set value must be a default heap payload",
            ));
        }
        scope
            .borrows()
            .borrow_unique(context.root.object_id(), context.root.type_id())?;
        let adapter = scope.runtime().host.path_adapter(context.descriptor.id)?;
        validate_path_operation(
            &adapter,
            &mut scope,
            &context,
            HostPathOperation::Set,
            Some(&value),
        )?;
        let old_value = adapter
            .read
            .as_ref()
            .map(|read| read(&mut scope, &context))
            .transpose()
            .map_err(|error| {
                RuntimeError::typed_path_validation(format!(
                    "host path previous-value read failed: {}",
                    error
                ))
            })?;
        scope
            .retain_temporaries(&old_value.iter().cloned().collect::<Vec<_>>())
            .map_err(path_scope_error)?;
        commit_path_write(
            &adapter,
            &mut scope,
            &context,
            HostPathMutationRecord {
                root: context.root,
                base_view: context.base_view.clone(),
                descriptor_id: context.descriptor.id,
                operation: HostPathOperation::Set,
                dynamic_args: context.dynamic_args.clone(),
                old_value,
                new_value: value,
            },
        )
    }

    pub(crate) fn modify_path(
        &mut self,
        root_or_view: &Value,
        descriptor_id: HostPathDescriptorId,
        dynamic_args: Vec<Value>,
        op: BinaryOp,
        value: Value,
    ) -> Result<Value, RuntimeError> {
        self.gc.ensure_execution_allowed()?;
        let mut scope = self.path_roots(root_or_view, &dynamic_args, Some(&value))?;
        let context = scope.runtime().host.resolve_path_context(
            scope.runtime().gc(),
            root_or_view,
            descriptor_id,
            dynamic_args,
            HostPathOperation::Modify(op),
        )?;
        if !value.is_default_heap_payload(scope.runtime().gc()) {
            return Err(RuntimeError::typed_path_validation(
                "path modify value must be a default heap payload",
            ));
        }
        scope
            .borrows()
            .borrow_unique(context.root.object_id(), context.root.type_id())?;
        let adapter = scope.runtime().host.path_adapter(context.descriptor.id)?;
        validate_path_operation(
            &adapter,
            &mut scope,
            &context,
            HostPathOperation::Modify(op),
            Some(&value),
        )?;
        let read = adapter.read.as_ref().ok_or_else(|| {
            RuntimeError::typed_path_validation("path descriptor does not support modify reads")
        })?;
        let old_value = read(&mut scope, &context).map_err(|error| {
            RuntimeError::typed_path_validation(format!("host path modify read failed: {}", error))
        })?;
        let new_value = apply_path_modify(op, old_value, value)?;
        scope
            .retain_temporaries(&[old_value, new_value])
            .map_err(path_scope_error)?;
        commit_path_write(
            &adapter,
            &mut scope,
            &context,
            HostPathMutationRecord {
                root: context.root,
                base_view: context.base_view.clone(),
                descriptor_id: context.descriptor.id,
                operation: HostPathOperation::Modify(op),
                dynamic_args: context.dynamic_args.clone(),
                old_value: Some(old_value),
                new_value,
            },
        )?;
        Ok(new_value)
    }

    fn path_roots<'a>(
        &'a mut self,
        root: &Value,
        args: &[Value],
        value: Option<&Value>,
    ) -> Result<HostCallContext<'a>, RuntimeError> {
        let values = iter::once(root)
            .chain(args)
            .chain(value)
            .cloned()
            .collect::<Vec<_>>();
        self.host_scope(&values)
            .map(|scope| HostCallContext { scope })
            .map_err(path_scope_error)
    }
}

fn validate_path_operation(
    adapter: &HostPathAdapter,
    call: &mut HostCallContext<'_>,
    context: &HostPathContext,
    operation: HostPathOperation,
    value: Option<&Value>,
) -> Result<(), RuntimeError> {
    if let Some(validate) = &adapter.validate {
        validate(call, context, operation, value).map_err(|error| {
            RuntimeError::typed_path_validation(format!("host path validation failed: {}", error))
        })?;
    }
    Ok(())
}

fn commit_path_write(
    adapter: &HostPathAdapter,
    call: &mut HostCallContext<'_>,
    context: &HostPathContext,
    record: HostPathMutationRecord,
) -> Result<(), RuntimeError> {
    if record
        .old_value
        .iter()
        .any(|value| !value.is_default_heap_payload(call.runtime().gc()))
    {
        return Err(RuntimeError::typed_path_validation(
            "previous path value cannot escape into a dirty record",
        ));
    }
    let prepare = adapter.prepare_write.as_ref().ok_or_else(|| {
        RuntimeError::typed_path_validation("path descriptor does not support writes")
    })?;
    // No registry or resource borrow spans preparation: it may collect GC.
    let prepared = prepare(call, context, &record);
    call.runtime().gc().ensure_execution_allowed()?;
    let prepared = prepared.map_err(|error| {
        RuntimeError::typed_path_validation(format!("host path preparation failed: {}", error))
    })?;
    let runtime = call.runtime_mut();
    let gc = &runtime.gc;
    let dirty = &mut runtime.host.dirty_paths;
    gc.prepare_dirty_record(dirty.len())?;
    dirty
        .try_reserve(1)
        .map_err(|_| gc.resource_limit("dirty record capacity"))?;
    gc.commit_host_write(move || {
        prepared.commit();
        dirty.push(record);
    })
}
