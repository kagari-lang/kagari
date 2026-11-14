//! Path callbacks borrow source values; only committed dirty records escape.
use crate::{
    Runtime,
    error::RuntimeError,
    host::{
        DynamicPathArgument, DynamicPathArguments, HostCallContext, HostPathAdapter,
        HostPathContext, HostPathDescriptorId, HostPathMutationRecord, HostPathOperation,
        apply_path_modify, path_scope_error,
    },
    value::{carriers::RootedValue, view::ValueView},
};
use kagari_bytecode::instruction::BinaryOp;

impl Runtime {
    pub fn read_host_path(
        &mut self,
        root: &RootedValue,
        descriptor: HostPathDescriptorId,
        arguments: &[RootedValue],
    ) -> Result<RootedValue, RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        self.reject_candidate_external_access()?;
        let mut call = HostCallContext::new(self, &[]).map_err(path_scope_error)?;
        let (root, args, _) = call.import_path_inputs(root, arguments, None)?;
        let value = call.read_path(&root, descriptor, &args);
        call.runtime().resources.ensure_execution_allowed()?;
        call.retain(&value?)
    }

    pub fn set_host_path(
        &mut self,
        root: &RootedValue,
        descriptor: HostPathDescriptorId,
        arguments: &[RootedValue],
        value: &RootedValue,
    ) -> Result<(), RuntimeError> {
        self.resources.poll_execution()?;
        self.reject_candidate_external_access()?;
        let mut call = HostCallContext::new(self, &[]).map_err(path_scope_error)?;
        let (root, args, value) = call.import_path_inputs(root, arguments, Some(value))?;
        let result = call.set_path(&root, descriptor, &args, &value.expect("write input"));
        call.runtime().resources.ensure_execution_allowed()?;
        result
    }

    pub fn modify_host_path(
        &mut self,
        root: &RootedValue,
        descriptor: HostPathDescriptorId,
        arguments: &[RootedValue],
        operation: BinaryOp,
        value: &RootedValue,
    ) -> Result<RootedValue, RuntimeError> {
        self.resources.poll_execution()?;
        self.reject_candidate_external_access()?;
        let mut call = HostCallContext::new(self, &[]).map_err(path_scope_error)?;
        let (root, args, value) = call.import_path_inputs(root, arguments, Some(value))?;
        let result = call.modify_path(
            &root,
            descriptor,
            &args,
            operation,
            &value.expect("modify input"),
        );
        call.runtime().resources.ensure_execution_allowed()?;
        call.retain(&result?)
    }
}

impl<'call> HostCallContext<'call> {
    fn import_path_inputs(
        &mut self,
        root: &RootedValue,
        arguments: &[RootedValue],
        value: Option<&RootedValue>,
    ) -> Result<
        (
            ValueView<'call>,
            Vec<ValueView<'call>>,
            Option<ValueView<'call>>,
        ),
        RuntimeError,
    > {
        let root = self.import(root).map_err(path_scope_error)?;
        let mut args = Vec::new();
        args.try_reserve_exact(arguments.len())
            .map_err(|_| RuntimeError::resource_limit("path arguments"))?;
        for argument in arguments {
            args.push(self.import(argument).map_err(path_scope_error)?);
        }
        let value = value
            .map(|value| self.import(value).map_err(path_scope_error))
            .transpose()?;
        Ok((root, args, value))
    }

    /// Snapshot the whole original pack before any source growth can make an
    /// expired argument coordinate valid. Reentry cannot overwrite these copies.
    pub(crate) fn snapshot_path_inputs(
        &mut self,
        root: &ValueView<'_>,
        arguments: &[ValueView<'_>],
        value: Option<&ValueView<'_>>,
    ) -> Result<
        (
            ValueView<'call>,
            Vec<ValueView<'call>>,
            Option<ValueView<'call>>,
        ),
        RuntimeError,
    > {
        let mut input = Vec::new();
        input
            .try_reserve_exact(
                arguments
                    .len()
                    .checked_add(2)
                    .ok_or_else(|| RuntimeError::resource_limit("path arguments"))?,
            )
            .map_err(|_| RuntimeError::resource_limit("path arguments"))?;
        input.push(root.reborrow());
        input.extend(arguments.iter().map(|value| value.reborrow()));
        input.extend(value.map(|value| value.reborrow()));
        let pack = self.retain_temporaries(&input).map_err(path_scope_error)?;
        let root = self.scope.argument(pack, 0)?;
        let arguments = input
            .into_iter()
            .skip(1)
            .take(arguments.len())
            .enumerate()
            .map(|(index, _)| self.scope.argument(pack, index + 1))
            .collect::<Result<Vec<_>, _>>()?;
        let value = value
            .map(|_| self.scope.argument(pack, arguments.len() + 1))
            .transpose()?;
        Ok((root, arguments, value))
    }

    pub(crate) fn read_path(
        &mut self,
        root: &ValueView<'call>,
        descriptor: HostPathDescriptorId,
        arguments: &[ValueView<'call>],
    ) -> Result<ValueView<'call>, RuntimeError> {
        let context = self.runtime_mut().prepare_path_context(
            root,
            descriptor,
            arguments,
            HostPathOperation::Read,
        )?;
        self.borrows()
            .borrow_shared(context.root.object_id(), context.root.type_id())?;
        let adapter = self.runtime().host.path_adapter(context.descriptor.id)?;
        validate_path_operation(&adapter, self, &context, HostPathOperation::Read, None)?;
        let read = adapter
            .read
            .as_ref()
            .ok_or_else(|| invalid("path descriptor does not support reads"))?;
        let value = read(self, &context)
            .map_err(|error| invalid(format!("host path read failed: {error}")))?;
        // Protection validates the returned source after callback/reentry; result
        // escape checks happen before the enclosing scope or frame retires it.
        let value = self.protect(&value).map_err(path_scope_error)?;
        self.scope.validate_result(&value)?;
        Ok(value)
    }

    pub(crate) fn set_path(
        &mut self,
        root: &ValueView<'call>,
        descriptor: HostPathDescriptorId,
        arguments: &[ValueView<'call>],
        value: &ValueView<'call>,
    ) -> Result<(), RuntimeError> {
        let context = self.runtime_mut().prepare_path_context(
            root,
            descriptor,
            arguments,
            HostPathOperation::Set,
        )?;
        self.require_storable(value)?;
        self.borrows()
            .borrow_unique(context.root.object_id(), context.root.type_id())?;
        let adapter = self.runtime().host.path_adapter(context.descriptor.id)?;
        validate_path_operation(
            &adapter,
            self,
            &context,
            HostPathOperation::Set,
            Some(value),
        )?;
        let old_value = adapter
            .read
            .as_ref()
            .map(|read| read(self, &context))
            .transpose()
            .map_err(|error| invalid(format!("host path previous-value read failed: {error}")))?
            .map(|value| self.protect(&value))
            .transpose()?;
        commit_path_write(
            &adapter,
            self,
            &context,
            HostPathMutationRecord {
                root: context.root,
                base_view: context.base_view,
                descriptor_id: context.descriptor.id,
                operation: HostPathOperation::Set,
                dynamic_args: context.dynamic_args.clone(),
                old_value,
                new_value: *value,
            },
        )
    }

    pub(crate) fn modify_path(
        &mut self,
        root: &ValueView<'call>,
        descriptor: HostPathDescriptorId,
        arguments: &[ValueView<'call>],
        operation: BinaryOp,
        value: &ValueView<'call>,
    ) -> Result<ValueView<'call>, RuntimeError> {
        let context = self.runtime_mut().prepare_path_context(
            root,
            descriptor,
            arguments,
            HostPathOperation::Modify(operation),
        )?;
        self.require_storable(value)?;
        self.borrows()
            .borrow_unique(context.root.object_id(), context.root.type_id())?;
        let adapter = self.runtime().host.path_adapter(context.descriptor.id)?;
        validate_path_operation(
            &adapter,
            self,
            &context,
            HostPathOperation::Modify(operation),
            Some(value),
        )?;
        let read = adapter
            .read
            .as_ref()
            .ok_or_else(|| invalid("path descriptor does not support modify reads"))?;
        let old_value = read(self, &context)
            .map_err(|error| invalid(format!("host path modify read failed: {error}")))?;
        let old_value = self.protect(&old_value)?;
        let new_value = apply_path_modify(operation, old_value, *value)?;
        commit_path_write(
            &adapter,
            self,
            &context,
            HostPathMutationRecord {
                root: context.root,
                base_view: context.base_view,
                descriptor_id: context.descriptor.id,
                operation: HostPathOperation::Modify(operation),
                dynamic_args: context.dynamic_args.clone(),
                old_value: Some(old_value),
                new_value,
            },
        )?;
        Ok(new_value)
    }
}

fn validate_path_operation<'call>(
    adapter: &HostPathAdapter,
    call: &mut HostCallContext<'call>,
    context: &HostPathContext<ValueView<'call>>,
    operation: HostPathOperation,
    value: Option<&ValueView<'call>>,
) -> Result<(), RuntimeError> {
    if let Some(validate) = &adapter.validate {
        validate(call, context, operation, value)
            .map_err(|error| invalid(format!("host path validation failed: {error}")))?;
    }
    Ok(())
}

fn commit_path_write<'call>(
    adapter: &HostPathAdapter,
    call: &mut HostCallContext<'call>,
    context: &HostPathContext<ValueView<'call>>,
    record: HostPathMutationRecord<ValueView<'call>>,
) -> Result<(), RuntimeError> {
    if let Some(value) = &record.old_value {
        call.require_storable(value)?;
    }
    let prepare = adapter
        .prepare_write
        .as_ref()
        .ok_or_else(|| invalid("path descriptor does not support writes"))?;
    // The callback can collect or reenter; no registry/source/payload loan spans it.
    let prepared = prepare(call, context, &record);
    call.runtime().resources.ensure_execution_allowed()?;
    let prepared =
        prepared.map_err(|error| invalid(format!("host path preparation failed: {error}")))?;
    if !call.runtime().host.matches_root(record.root) {
        return Err(invalid("host root changed during path preparation"));
    }
    let record = retain_record(call, record)?;
    let runtime = call.runtime_mut();
    runtime
        .resources
        .prepare_dirty_record(runtime.host.dirty_paths.len())?;
    runtime
        .host
        .dirty_paths
        .try_reserve(1)
        .map_err(|_| RuntimeError::resource_limit("dirty record capacity"))?;
    let dirty = &mut runtime.host.dirty_paths;
    runtime.resources.commit_host_write(move || {
        prepared.commit();
        dirty.push(record);
    })
}

fn retain_record(
    call: &mut HostCallContext<'_>,
    record: HostPathMutationRecord<ValueView<'_>>,
) -> Result<HostPathMutationRecord<RootedValue>, RuntimeError> {
    let mut arguments = Vec::new();
    arguments
        .try_reserve_exact(record.dynamic_args.len())
        .map_err(|_| RuntimeError::resource_limit("dirty path arguments"))?;
    for argument in record.dynamic_args.as_slice() {
        call.require_storable(&argument.value)?;
        arguments.push(DynamicPathArgument::new(
            argument.ty,
            call.retain(&argument.value)?,
        ));
    }
    Ok(HostPathMutationRecord {
        root: record.root,
        descriptor_id: record.descriptor_id,
        operation: record.operation,
        base_view: record
            .base_view
            .map(|value| call.retain(&value))
            .transpose()?,
        dynamic_args: DynamicPathArguments::new(arguments),
        old_value: record
            .old_value
            .map(|value| call.retain(&value))
            .transpose()?,
        new_value: call.retain(&record.new_value)?,
    })
}

fn invalid(message: impl Into<String>) -> RuntimeError {
    RuntimeError::typed_path_validation(message)
}
