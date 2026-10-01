use crate::{
    Runtime,
    error::RuntimeError,
    gc::GcHeap,
    host::{
        DynamicPathArgSlot, DynamicPathArguments, HostBorrowTable, HostCallContext, HostFunction,
        HostFunctionId, HostObjectId, HostPathAdapter, HostPathContext, HostPathDescriptor,
        HostPathDescriptorId, HostPathDescriptorRegistration, HostPathMutationRecord,
        HostPathOperation, HostPathSegment, HostPathSegmentRegistration, HostPathViewHandle,
        HostRegistry, HostRegistryId, HostRootHandle, HostSchemaEpoch, HostTypeInfo,
        apply_path_modify, dynamic_args_for_descriptor, path_access_allows, path_scope_error,
        validate_dynamic_arguments, validate_path_access,
    },
    metadata::{TypeId, TypeRegistry},
    value::Value,
};
use kagari_bytecode::instruction::BinaryOp;
use kagari_common::{
    host_interface::{
        self, HostInterface,
        path::{HostPathDeclaration, HostPathSegmentDeclaration},
        type_declaration::{HostTypeOwnership, PathAccess, Visibility},
        value_type::HostValueType,
    },
    identity::DefinitionId,
};
use std::iter;

impl HostRegistry {
    pub(crate) fn gc_roots(&self) -> Vec<Value> {
        let mut roots = Vec::new();
        for record in self.dirty_paths.borrow().iter() {
            roots.extend(record.old_value.iter().cloned());
            roots.push(record.new_value.clone());
            roots.extend(
                record
                    .dynamic_args
                    .as_slice()
                    .iter()
                    .map(|arg| arg.value.clone()),
            );
            roots.extend(record.base_view.iter().cloned().map(Value::HostPathView));
        }
        roots
    }

    pub(crate) fn owner(&self) -> HostRegistryId {
        self.owner
    }

    pub fn bound_function(&self, id: HostFunctionId) -> Option<&HostFunction> {
        (id.owner == self.owner)
            .then(|| self.functions.get(id.slot))
            .flatten()
    }

    pub fn register(&mut self, mut function: HostFunction) -> Result<HostFunctionId, RuntimeError> {
        function
            .declaration
            .validate()
            .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;
        if let Some(owner) = function.declaration.method_owner() {
            let ty = self.host_type_by_declaration(&owner).ok_or_else(|| {
                RuntimeError::metadata_conflict("host method owner is not registered")
            })?;
            let required = ty
                .declaration
                .method_contract(&function.declaration.id)
                .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;
            if !required.matches_binding(&function.declaration) {
                return Err(RuntimeError::metadata_conflict(
                    "host method binding differs from its member declaration",
                ));
            }
        }
        let symbol = function.declaration.symbol.to_owned();
        if self.function_names.contains_key(&symbol)
            || self
                .functions
                .iter()
                .any(|existing| existing.declaration.id == function.declaration.id)
        {
            return Err(RuntimeError::metadata_conflict(symbol));
        }
        let id = HostFunctionId {
            owner: self.owner,
            slot: self.functions.len(),
        };
        function.assign_id(id);
        self.function_names.insert(symbol, id);
        self.function_declarations
            .insert(function.declaration.id.clone(), id);
        self.functions.push(function);
        Ok(id)
    }

    pub fn interface(&self) -> HostInterface {
        let mut paths = Vec::new();
        for declaration in self.path_descriptors().map(|path| &path.declaration) {
            if !paths.contains(declaration) {
                paths.push(declaration.clone());
            }
        }
        let mut functions = self
            .functions
            .iter()
            .map(|f| f.declaration.clone())
            .collect::<Vec<_>>();
        functions.sort_by(|a, b| a.id.cmp(&b.id));
        let mut types = self
            .types
            .values()
            .map(|info| info.declaration.clone())
            .collect::<Vec<_>>();
        types.sort_by(|a, b| a.id.cmp(&b.id));
        HostInterface {
            paths,
            types,
            functions,
        }
    }

    /// Checks declarations without invoking any callback or changing registry state.
    pub fn link_interface(
        &self,
        interface: &HostInterface,
    ) -> Result<Vec<HostFunctionId>, RuntimeError> {
        interface
            .validate()
            .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;
        for required in &interface.paths {
            let fingerprint = required
                .contract(interface)
                .and_then(|contract| contract.fingerprint())
                .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;
            if self
                .path_descriptors()
                .filter(|path| {
                    path.abi_fingerprint.0 == fingerprint && &path.declaration == required
                })
                .count()
                != 1
            {
                return Err(RuntimeError::typed_path_validation(
                    "required host path has missing or ambiguous bindings",
                ));
            }
        }
        for required in &interface.types {
            let actual = self.host_type_by_declaration(&required.id).ok_or_else(|| {
                RuntimeError::metadata_conflict(format!(
                    "missing host type binding `{}`",
                    required.symbol
                ))
            })?;
            if !required.matches_binding(&actual.declaration) {
                return Err(RuntimeError::metadata_conflict(format!(
                    "host type binding `{}` differs from its declaration",
                    required.symbol
                )));
            }
            for implementation in &required.trait_implementations {
                for method in &implementation.methods {
                    let bound = self
                        .function_declarations
                        .get(&method.host_method)
                        .and_then(|id| self.bound_function(*id))
                        .ok_or_else(|| {
                            RuntimeError::metadata_conflict(format!(
                                "missing host trait method binding `{}`",
                                method.host_method.path.last().map_or("", |part| &part.name)
                            ))
                        })?;
                    let declared = actual
                        .declaration
                        .method_contract(&method.host_method)
                        .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;
                    if !declared.matches_binding(&bound.declaration) {
                        return Err(RuntimeError::metadata_conflict(
                            "host trait method binding differs from its declaration",
                        ));
                    }
                }
            }
        }
        interface
            .functions
            .iter()
            .map(|required| {
                let bound = self
                    .function_declarations
                    .get(&required.id)
                    .and_then(|id| self.bound_function(*id))
                    .ok_or_else(|| {
                        RuntimeError::metadata_conflict(format!(
                            "missing host binding `{}`",
                            required.symbol
                        ))
                    })?;
                if !required.matches_binding(&bound.declaration) {
                    return Err(RuntimeError::metadata_conflict(format!(
                        "host binding `{}` differs from its declaration",
                        required.symbol
                    )));
                }
                bound
                    .id
                    .ok_or_else(|| RuntimeError::metadata_conflict("unregistered host binding"))
            })
            .collect()
    }

    pub(crate) fn install_types(&mut self, bindings: Vec<HostTypeInfo>) {
        for info in bindings {
            self.type_names
                .insert(info.declaration.symbol.clone(), info.type_id);
            self.type_declarations
                .insert(info.declaration.id.clone(), info.type_id);
            self.types.insert(info.type_id, info);
        }
    }

    pub(crate) fn validate_type_identity(
        &self,
        declaration: &DefinitionId,
        symbol: &str,
    ) -> Result<(), RuntimeError> {
        host_interface::validate_host_type_identity(declaration)
            .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;
        if symbol.is_empty()
            || symbol.split('.').any(str::is_empty)
            || self.type_declarations.contains_key(declaration)
            || self.type_names.contains_key(symbol)
        {
            return Err(RuntimeError::metadata_conflict(
                "invalid or duplicate host type declaration",
            ));
        }
        Ok(())
    }

    pub(super) fn matches_type(&self, type_id: TypeId, declaration: &DefinitionId) -> bool {
        self.type_declarations.get(declaration) == Some(&type_id)
    }

    pub(crate) fn matches_root(&self, root: HostRootHandle) -> bool {
        root.owner == self.owner && self.roots.get(&root.object_id) == Some(&root)
    }

    pub fn register_root(
        &mut self,
        object_id: HostObjectId,
        type_id: TypeId,
        schema_epoch: HostSchemaEpoch,
    ) -> Result<HostRootHandle, RuntimeError> {
        if self.roots.contains_key(&object_id) {
            return Err(RuntimeError::metadata_conflict(format!(
                "host root {}",
                object_id.0
            )));
        }
        let Some(info) = self.types.get(&type_id) else {
            return Err(RuntimeError::typed_path_validation(
                "host root type is not registered",
            ));
        };
        if info.declaration.ownership != HostTypeOwnership::HostRoot {
            return Err(RuntimeError::typed_path_validation(
                "host root type must use HostRoot ownership",
            ));
        }
        validate_path_access(info.declaration.path_access, "host root type")?;
        let root = HostRootHandle::new(
            self.owner,
            object_id,
            type_id,
            schema_epoch,
            info.abi_fingerprint,
        );
        self.roots.insert(object_id, root);
        Ok(root)
    }

    pub fn root(&self, object_id: HostObjectId) -> Option<HostRootHandle> {
        self.roots.get(&object_id).copied()
    }

    pub fn roots(&self) -> impl Iterator<Item = HostRootHandle> + '_ {
        self.roots.values().copied()
    }

    pub(super) fn portable_path_type(
        &self,
        declaration: &HostValueType,
        types: &TypeRegistry,
    ) -> Result<TypeId, RuntimeError> {
        let resolved = match declaration {
            HostValueType::Opaque(id) => self.host_type_by_declaration(id).map(|ty| ty.type_id),
            ty => types.host_value_type_id(ty),
        };
        resolved.ok_or_else(|| {
            RuntimeError::typed_path_validation("path segment type has no registered declaration")
        })
    }

    pub(crate) fn register_path_descriptor(
        &mut self,
        registration: HostPathDescriptorRegistration,
        types: &TypeRegistry,
    ) -> Result<HostPathDescriptorId, RuntimeError> {
        let Some(root_type) = self.types.get(&registration.root_type) else {
            return Err(RuntimeError::typed_path_validation(
                "path descriptor root type is not registered",
            ));
        };
        if root_type.declaration.ownership != HostTypeOwnership::HostRoot {
            return Err(RuntimeError::typed_path_validation(
                "path descriptor root type must use HostRoot ownership",
            ));
        }
        if !path_access_allows(root_type.declaration.path_access, registration.access) {
            return Err(RuntimeError::typed_path_validation(
                "path descriptor access exceeds root type policy",
            ));
        }

        let mut current = registration.root_type;
        let mut segments = Vec::with_capacity(registration.segments.len());
        for segment in &registration.segments {
            let resolved = match segment {
                HostPathSegmentRegistration::Field { declaration } => {
                    let owner = self.types.get(&current).ok_or_else(|| {
                        RuntimeError::typed_path_validation(
                            "field owner is not a declared host type",
                        )
                    })?;
                    let slot = owner
                        .declaration
                        .fields
                        .iter()
                        .position(|field| &field.id == declaration)
                        .ok_or_else(|| {
                            RuntimeError::typed_path_validation(
                                "field declaration does not belong to path owner",
                            )
                        })?;
                    let metadata = types.get(current).ok_or_else(|| {
                        RuntimeError::typed_path_validation("missing host type metadata")
                    })?;
                    let field = metadata.fields.get(slot).ok_or_else(|| {
                        RuntimeError::typed_path_validation("missing host field metadata")
                    })?;
                    if field.visibility != Visibility::Public {
                        return Err(RuntimeError::typed_path_validation(
                            "private host fields cannot be exposed as paths",
                        ));
                    }
                    HostPathSegment::Field {
                        name: field.name.clone(),
                        field_id: field.id,
                        owner_type: current,
                        result_type: field.ty,
                        access: field.path_access,
                        abi_fingerprint: field.abi_fingerprint,
                    }
                }
                HostPathSegmentRegistration::Index { declaration } => {
                    declaration
                        .validate()
                        .map_err(|error| RuntimeError::typed_path_validation(error.to_string()))?;
                    HostPathSegment::Index {
                        slot: DynamicPathArgSlot::new(declaration.slot as usize),
                        collection_type: self.portable_path_type(&declaration.collection, types)?,
                        index_type: self.portable_path_type(&declaration.index, types)?,
                        result_type: self.portable_path_type(&declaration.result, types)?,
                        access: declaration.access,
                    }
                }
                HostPathSegmentRegistration::Virtual { declaration } => {
                    declaration
                        .validate()
                        .map_err(|error| RuntimeError::typed_path_validation(error.to_string()))?;
                    HostPathSegment::Virtual {
                        name: declaration.name.clone(),
                        result_type: self.portable_path_type(&declaration.result, types)?,
                        access: declaration.access,
                    }
                }
            };
            current = resolved.result_type();
            segments.push(resolved);
        }
        let id = HostPathDescriptorId::new(self.next_path_descriptor_id);
        let declaration = HostPathDeclaration {
            root: root_type.declaration.id.clone(),
            segments: registration
                .segments
                .iter()
                .map(|segment| match segment {
                    HostPathSegmentRegistration::Field { declaration } => {
                        HostPathSegmentDeclaration::Field(declaration.clone())
                    }
                    HostPathSegmentRegistration::Index { declaration } => {
                        HostPathSegmentDeclaration::Index(declaration.clone())
                    }
                    HostPathSegmentRegistration::Virtual { declaration } => {
                        HostPathSegmentDeclaration::Virtual(declaration.clone())
                    }
                })
                .collect(),
            access: registration.access,
            schema_epoch: registration.schema_epoch.0,
            capabilities: registration.capability_requirements,
        };
        let fingerprint = self.path_fingerprint(&registration, &segments, types)?;
        let offline_fingerprint = declaration
            .contract(&self.interface())
            .and_then(|contract| contract.fingerprint())
            .map_err(|error| RuntimeError::typed_path_validation(error.to_string()))?;
        if fingerprint.0 != offline_fingerprint {
            return Err(RuntimeError::typed_path_validation(
                "runtime path contract differs from its portable declaration",
            ));
        }
        let descriptor = HostPathDescriptor::from_registration(
            id,
            registration,
            segments,
            fingerprint,
            declaration,
        )?;
        self.next_path_descriptor_id += 1;
        self.path_descriptors.insert(id, descriptor);
        Ok(id)
    }

    pub(crate) fn register_path(
        &mut self,
        declaration: &HostPathDeclaration,
        types: &TypeRegistry,
    ) -> Result<HostPathDescriptorId, RuntimeError> {
        let contract = declaration
            .contract(&self.interface())
            .map_err(|error| RuntimeError::typed_path_validation(error.to_string()))?;
        let root_type = self
            .host_type_by_declaration(&declaration.root)
            .ok_or_else(|| RuntimeError::typed_path_validation("missing host path root"))?
            .type_id;
        let result_type = self.portable_path_type(&contract.result, types)?;
        self.register_path_descriptor(
            HostPathDescriptorRegistration {
                root_type,
                result_type,
                segments: declaration
                    .segments
                    .iter()
                    .map(|segment| match segment {
                        HostPathSegmentDeclaration::Field(id) => {
                            HostPathSegmentRegistration::Field {
                                declaration: id.clone(),
                            }
                        }
                        HostPathSegmentDeclaration::Index(index) => {
                            HostPathSegmentRegistration::Index {
                                declaration: index.clone(),
                            }
                        }
                        HostPathSegmentDeclaration::Virtual(virtual_step) => {
                            HostPathSegmentRegistration::Virtual {
                                declaration: virtual_step.clone(),
                            }
                        }
                    })
                    .collect(),
                access: declaration.access,
                schema_epoch: HostSchemaEpoch(declaration.schema_epoch),
                capability_requirements: declaration.capabilities,
            },
            types,
        )
    }

    pub fn register_path_adapter(
        &mut self,
        descriptor_id: HostPathDescriptorId,
        adapter: HostPathAdapter,
    ) -> Result<(), RuntimeError> {
        if !self.path_descriptors.contains_key(&descriptor_id) {
            return Err(RuntimeError::typed_path_validation(
                "path adapter descriptor is not registered",
            ));
        }
        self.path_adapters.insert(descriptor_id, adapter);
        Ok(())
    }

    pub fn path_descriptor(&self, id: HostPathDescriptorId) -> Option<&HostPathDescriptor> {
        self.path_descriptors.get(&id)
    }

    pub fn path_descriptors(&self) -> impl Iterator<Item = &HostPathDescriptor> {
        self.path_descriptors.values()
    }

    pub fn make_path_view(
        &self,
        root: HostRootHandle,
        descriptor_id: HostPathDescriptorId,
        dynamic_args: DynamicPathArguments,
    ) -> Result<HostPathViewHandle, RuntimeError> {
        let registered_root = self
            .roots
            .get(&root.object_id)
            .ok_or_else(|| RuntimeError::typed_path_validation("host root is not registered"))?;
        if *registered_root != root {
            return Err(RuntimeError::typed_path_validation(
                "host root handle does not match registered root metadata",
            ));
        }
        let descriptor = self.path_descriptors.get(&descriptor_id).ok_or_else(|| {
            RuntimeError::typed_path_validation("path descriptor is not registered")
        })?;
        if descriptor.root_type != root.type_id {
            return Err(RuntimeError::typed_path_validation(
                "path descriptor root type does not match host root type",
            ));
        }
        if descriptor.schema_epoch != root.schema_epoch {
            return Err(RuntimeError::typed_path_validation(
                "path descriptor schema epoch does not match host root epoch",
            ));
        }
        validate_dynamic_arguments(descriptor, &dynamic_args)?;
        Ok(HostPathViewHandle::new(
            root,
            None,
            descriptor,
            dynamic_args,
        ))
    }

    pub fn make_path_view_from_value(
        &self,
        root_or_view: &Value,
        descriptor_id: HostPathDescriptorId,
        dynamic_args: Vec<Value>,
    ) -> Result<HostPathViewHandle, RuntimeError> {
        let context = self.resolve_path_context(
            root_or_view,
            descriptor_id,
            dynamic_args,
            HostPathOperation::MakeView,
        )?;
        Ok(HostPathViewHandle::new(
            context.root,
            context.base_view,
            &context.descriptor,
            context.dynamic_args,
        ))
    }

    pub(crate) fn read_path(
        &self,
        runtime: &Runtime,
        root_or_view: &Value,
        descriptor_id: HostPathDescriptorId,
        dynamic_args: Vec<Value>,
    ) -> Result<Value, RuntimeError> {
        let gc = runtime.gc();
        gc.ensure_execution_allowed()?;
        let scope = Self::path_roots(runtime, root_or_view, &dynamic_args, None)?;
        let context = self.resolve_path_context(
            root_or_view,
            descriptor_id,
            dynamic_args,
            HostPathOperation::Read,
        )?;
        scope
            .borrows()
            .borrow_shared(context.root.object_id(), context.root.type_id())?;
        let adapter = self.path_adapter(context.descriptor.id)?;
        self.validate_path_operation(&adapter, &scope, &context, HostPathOperation::Read, None)?;
        let read = adapter.read.as_ref().ok_or_else(|| {
            RuntimeError::typed_path_validation("path descriptor does not support reads")
        })?;
        let value = read(&scope, &context).map_err(|error| {
            RuntimeError::typed_path_validation(format!(
                "host path read failed: {}",
                error.message()
            ))
        })?;
        HostBorrowTable::validate_no_escape(&value)?;
        if !gc.validate_value(&value) {
            return Err(RuntimeError::typed_path_validation(
                "invalid heap reference in path result",
            ));
        }
        Ok(value)
    }

    pub(crate) fn set_path(
        &self,
        runtime: &Runtime,
        root_or_view: &Value,
        descriptor_id: HostPathDescriptorId,
        dynamic_args: Vec<Value>,
        value: Value,
    ) -> Result<(), RuntimeError> {
        let gc = runtime.gc();
        gc.ensure_execution_allowed()?;
        let scope = Self::path_roots(runtime, root_or_view, &dynamic_args, Some(&value))?;
        let context = self.resolve_path_context(
            root_or_view,
            descriptor_id,
            dynamic_args,
            HostPathOperation::Set,
        )?;
        if !value.is_default_heap_payload() {
            return Err(RuntimeError::typed_path_validation(
                "path set value must be a default heap payload",
            ));
        }
        scope
            .borrows()
            .borrow_unique(context.root.object_id(), context.root.type_id())?;
        let adapter = self.path_adapter(context.descriptor.id)?;
        self.validate_path_operation(
            &adapter,
            &scope,
            &context,
            HostPathOperation::Set,
            Some(&value),
        )?;
        let old_value = adapter
            .read
            .as_ref()
            .map(|read| read(&scope, &context))
            .transpose()
            .map_err(|error| {
                RuntimeError::typed_path_validation(format!(
                    "host path previous-value read failed: {}",
                    error.message()
                ))
            })?;
        scope
            .retain_temporaries(&old_value.iter().cloned().collect::<Vec<_>>())
            .map_err(path_scope_error)?;
        self.commit_path_write(
            gc,
            &adapter,
            &scope,
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
        &self,
        runtime: &Runtime,
        root_or_view: &Value,
        descriptor_id: HostPathDescriptorId,
        dynamic_args: Vec<Value>,
        op: BinaryOp,
        value: Value,
    ) -> Result<Value, RuntimeError> {
        let gc = runtime.gc();
        gc.ensure_execution_allowed()?;
        let scope = Self::path_roots(runtime, root_or_view, &dynamic_args, Some(&value))?;
        let context = self.resolve_path_context(
            root_or_view,
            descriptor_id,
            dynamic_args,
            HostPathOperation::Modify(op),
        )?;
        if !value.is_default_heap_payload() {
            return Err(RuntimeError::typed_path_validation(
                "path modify value must be a default heap payload",
            ));
        }
        scope
            .borrows()
            .borrow_unique(context.root.object_id(), context.root.type_id())?;
        let adapter = self.path_adapter(context.descriptor.id)?;
        self.validate_path_operation(
            &adapter,
            &scope,
            &context,
            HostPathOperation::Modify(op),
            Some(&value),
        )?;
        let read = adapter.read.as_ref().ok_or_else(|| {
            RuntimeError::typed_path_validation("path descriptor does not support modify reads")
        })?;
        let old_value = read(&scope, &context).map_err(|error| {
            RuntimeError::typed_path_validation(format!(
                "host path modify read failed: {}",
                error.message()
            ))
        })?;
        let new_value = apply_path_modify(op, old_value.clone(), value)?;
        scope
            .retain_temporaries(&[old_value.clone(), new_value.clone()])
            .map_err(path_scope_error)?;
        self.commit_path_write(
            gc,
            &adapter,
            &scope,
            &context,
            HostPathMutationRecord {
                root: context.root,
                base_view: context.base_view.clone(),
                descriptor_id: context.descriptor.id,
                operation: HostPathOperation::Modify(op),
                dynamic_args: context.dynamic_args.clone(),
                old_value: Some(old_value),
                new_value: new_value.clone(),
            },
        )?;
        Ok(new_value)
    }

    pub fn dirty_paths(&self) -> Vec<HostPathMutationRecord> {
        self.dirty_paths.borrow().clone()
    }

    pub(super) fn path_roots<'a>(
        runtime: &'a Runtime,
        root: &Value,
        args: &[Value],
        value: Option<&Value>,
    ) -> Result<HostCallContext<'a>, RuntimeError> {
        let values = iter::once(root)
            .chain(args)
            .chain(value)
            .cloned()
            .collect::<Vec<_>>();
        runtime
            .host_scope(&values)
            .map(|scope| HostCallContext { scope })
            .map_err(path_scope_error)
    }

    pub fn clear_dirty_paths(&self) {
        self.dirty_paths.borrow_mut().clear();
    }

    pub(super) fn resolve_path_context(
        &self,
        root_or_view: &Value,
        descriptor_id: HostPathDescriptorId,
        dynamic_args: Vec<Value>,
        operation: HostPathOperation,
    ) -> Result<HostPathContext, RuntimeError> {
        let descriptor = self
            .path_descriptors
            .get(&descriptor_id)
            .cloned()
            .ok_or_else(|| {
                RuntimeError::typed_path_validation("path descriptor is not registered")
            })?;
        if operation.writes() && descriptor.access != PathAccess::ReadWrite {
            return Err(RuntimeError::typed_path_validation(
                "path descriptor is not writable",
            ));
        }
        let dynamic_args = dynamic_args_for_descriptor(&descriptor, dynamic_args)?;
        validate_dynamic_arguments(&descriptor, &dynamic_args)?;
        let (root, base_view) = match root_or_view {
            Value::HostRoot(root) => {
                let registered_root = self.roots.get(&root.object_id).ok_or_else(|| {
                    RuntimeError::typed_path_validation("host root is not registered")
                })?;
                if registered_root != root {
                    return Err(RuntimeError::typed_path_validation(
                        "host root handle does not match registered root metadata",
                    ));
                }
                if descriptor.root_type != root.type_id {
                    return Err(RuntimeError::typed_path_validation(
                        "path descriptor root type does not match host root type",
                    ));
                }
                if descriptor.schema_epoch != root.schema_epoch {
                    return Err(RuntimeError::typed_path_validation(
                        "path descriptor schema epoch does not match host root epoch",
                    ));
                }
                (*root, None)
            }
            Value::HostPathView(view) => {
                if !self.matches_root(view.root) {
                    return Err(RuntimeError::typed_path_validation(
                        "host path view belongs to another registry or has an unregistered root",
                    ));
                }
                if descriptor.root_type != view.result_type {
                    return Err(RuntimeError::typed_path_validation(
                        "path descriptor root type does not match host path view result type",
                    ));
                }
                if descriptor.schema_epoch != view.schema_epoch {
                    return Err(RuntimeError::typed_path_validation(
                        "path descriptor schema epoch does not match host path view epoch",
                    ));
                }
                (view.root, Some(view.clone()))
            }
            _ => {
                return Err(RuntimeError::typed_path_validation(
                    "path operation expects host root or host path view",
                ));
            }
        };
        Ok(HostPathContext {
            root,
            base_view,
            descriptor,
            dynamic_args,
        })
    }

    pub(super) fn path_adapter(
        &self,
        descriptor_id: HostPathDescriptorId,
    ) -> Result<HostPathAdapter, RuntimeError> {
        self.path_adapters
            .get(&descriptor_id)
            .cloned()
            .ok_or_else(|| RuntimeError::typed_path_validation("path descriptor has no adapter"))
    }

    pub(super) fn validate_path_operation(
        &self,
        adapter: &HostPathAdapter,
        call: &HostCallContext<'_>,
        context: &HostPathContext,
        operation: HostPathOperation,
        value: Option<&Value>,
    ) -> Result<(), RuntimeError> {
        if let Some(validate) = &adapter.validate {
            validate(call, context, operation, value).map_err(|error| {
                RuntimeError::typed_path_validation(format!(
                    "host path validation failed: {}",
                    error.message()
                ))
            })?;
        }
        Ok(())
    }

    pub(super) fn commit_path_write(
        &self,
        gc: &GcHeap,
        adapter: &HostPathAdapter,
        call: &HostCallContext<'_>,
        context: &HostPathContext,
        record: HostPathMutationRecord,
    ) -> Result<(), RuntimeError> {
        if record
            .old_value
            .iter()
            .any(|value| !value.is_default_heap_payload())
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
        gc.ensure_execution_allowed()?;
        let prepared = prepared.map_err(|error| {
            RuntimeError::typed_path_validation(format!(
                "host path preparation failed: {}",
                error.message()
            ))
        })?;
        let mut dirty = self.dirty_paths.borrow_mut();
        gc.prepare_dirty_record(dirty.len())?;
        dirty
            .try_reserve(1)
            .map_err(|_| gc.resource_limit("dirty record capacity"))?;
        gc.commit_host_write(move || {
            prepared.commit();
            dirty.push(record);
        })
    }

    pub fn function(&self, symbol: &str) -> Option<&HostFunction> {
        self.function_names
            .get(symbol)
            .and_then(|id| self.bound_function(*id))
    }

    pub fn functions(&self) -> impl Iterator<Item = &HostFunction> {
        self.functions.iter()
    }

    pub fn host_type(&self, type_id: TypeId) -> Option<&HostTypeInfo> {
        self.types.get(&type_id)
    }

    pub fn host_type_by_declaration(&self, declaration: &DefinitionId) -> Option<&HostTypeInfo> {
        self.types.get(self.type_declarations.get(declaration)?)
    }

    pub fn host_type_by_name(&self, script_name: &str) -> Option<&HostTypeInfo> {
        let type_id = self.type_names.get(script_name)?;
        self.types.get(type_id)
    }

    pub fn host_types(&self) -> impl Iterator<Item = &HostTypeInfo> {
        self.types.values()
    }
}
