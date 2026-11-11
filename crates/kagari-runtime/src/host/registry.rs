use crate::{
    error::RuntimeError,
    gc::GcHeap,
    host::{
        DynamicPathArgSlot, DynamicPathArguments, HostObjectId, HostPathAdapter, HostPathContext,
        HostPathDescriptor, HostPathDescriptorId, HostPathDescriptorRegistration,
        HostPathMutationRecord, HostPathOperation, HostPathSegment, HostPathViewHandle,
        HostRegistry, HostRegistryId, HostRootHandle, HostSchemaEpoch, dynamic_args_for_descriptor,
        path_access_allows, validate_dynamic_arguments, validate_path_access,
    },
    metadata::{TypeId, TypeRegistry},
    value::Value,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind,
        map::DefinitionContext,
        metadata::DefinitionMetadata,
        table::{DefinitionId, DefinitionTable},
    },
};

use kagari_types::{
    declaration::{
        host::{HostTypeOwnership, PathAccess},
        path::{HostPathDeclaration, HostPathSegmentDeclaration},
    },
    ty::Ty,
    visibility::Visibility,
};

impl Default for HostRegistry {
    fn default() -> Self {
        Self::new(
            DefinitionContext::new().expect("definition context allocation"),
            None,
        )
    }
}

impl HostRegistry {
    pub fn definitions(&self) -> DefinitionTable {
        self.definitions.snapshot()
    }

    pub(crate) fn gc_roots(&self) -> Vec<Value> {
        let mut roots = Vec::new();
        for record in self.dirty_paths.iter() {
            roots.extend(record.old_value.iter().cloned());
            roots.push(record.new_value);
            roots.extend(record.dynamic_args.as_slice().iter().map(|arg| arg.value));
            if let Some(view) = &record.base_view {
                roots.extend(view.retained_values().copied());
            }
        }
        roots
    }

    pub(crate) fn owner(&self) -> HostRegistryId {
        self.owner
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
        let Some(info) = self.registered_types.get(&type_id) else {
            return Err(RuntimeError::typed_path_validation(
                "host root type is not registered",
            ));
        };
        if info.policy.ownership != HostTypeOwnership::HostRoot {
            return Err(RuntimeError::typed_path_validation(
                "host root type must use HostRoot ownership",
            ));
        }
        validate_path_access(info.policy.path_access, "host root type")?;
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
        declaration: &Ty,
        types: &TypeRegistry,
    ) -> Result<TypeId, RuntimeError> {
        let resolved = match declaration {
            Ty::Host(id) => self.type_declarations.get(id).copied(),
            ty => types.path_type(ty),
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
        let mut registration = registration;
        let cancel = CancellationToken::default();
        let input =
            DefinitionMetadata::checked(registration.definitions, registration.segments, &cancel)
                .map_err(|error| RuntimeError::typed_path_validation(error.to_string()))?;
        let adopted = if input.definitions().id() == self.definitions().id() {
            input
        } else {
            input
                .import_into(&self.definitions, &cancel)
                .map_err(|error| RuntimeError::typed_path_validation(error.to_string()))?
        };
        (registration.definitions, registration.segments) = adopted.into_parts();
        let definitions = self.definitions();
        let Some(root_type) = self.registered_types.get(&registration.root_type) else {
            return Err(RuntimeError::typed_path_validation(
                "path descriptor root type is not registered",
            ));
        };
        if root_type.policy.ownership != HostTypeOwnership::HostRoot {
            return Err(RuntimeError::typed_path_validation(
                "path descriptor root type must use HostRoot ownership",
            ));
        }
        if !path_access_allows(root_type.policy.path_access, registration.access) {
            return Err(RuntimeError::typed_path_validation(
                "path descriptor access exceeds root type policy",
            ));
        }

        let mut current = registration.root_type;
        let mut segments = Vec::with_capacity(registration.segments.len());
        for segment in &registration.segments {
            let resolved = match segment {
                HostPathSegmentDeclaration::Field(declaration) => {
                    let owner = self
                        .type_declarations
                        .iter()
                        .find_map(|(declaration, id)| (*id == current).then_some(*declaration))
                        .ok_or_else(|| {
                            RuntimeError::typed_path_validation(
                                "field owner is not a declared host type",
                            )
                        })?;
                    let metadata = types.get(current).ok_or_else(|| {
                        RuntimeError::typed_path_validation("missing host type metadata")
                    })?;
                    let field = metadata
                        .fields
                        .iter()
                        .find(|field| {
                            definitions.lookup_child(owner, DefinitionKind::Field, &field.name, 0)
                                == Some(*declaration)
                        })
                        .ok_or_else(|| {
                            RuntimeError::typed_path_validation(
                                "field declaration does not belong to path owner",
                            )
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
                HostPathSegmentDeclaration::Index(declaration) => {
                    declaration
                        .validate(&definitions)
                        .map_err(|error| RuntimeError::typed_path_validation(error.to_string()))?;
                    HostPathSegment::Index {
                        slot: DynamicPathArgSlot::new(declaration.slot as usize),
                        collection_type: self.portable_path_type(&declaration.collection, types)?,
                        index_type: self.portable_path_type(&declaration.index, types)?,
                        result_type: self.portable_path_type(&declaration.result, types)?,
                        access: declaration.access,
                    }
                }
                HostPathSegmentDeclaration::Virtual(declaration) => {
                    declaration
                        .validate(&definitions)
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
            root: self
                .type_declarations
                .iter()
                .find_map(|(declaration, id)| {
                    (*id == registration.root_type).then_some(*declaration)
                })
                .ok_or_else(|| {
                    RuntimeError::typed_path_validation("path root declaration is missing")
                })?,
            segments: registration.segments.clone(),
            access: registration.access,
            schema_epoch: registration.schema_epoch.0,
        };
        let fingerprint = self.path_fingerprint(&registration, &segments, types)?;
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
        declaration: &DefinitionMetadata<HostPathDeclaration>,
        types: &TypeRegistry,
    ) -> Result<HostPathDescriptorId, RuntimeError> {
        let adopted = declaration
            .import_into(&self.definitions, &CancellationToken::default())
            .map_err(|error| RuntimeError::typed_path_validation(error.to_string()))?;
        let declaration = adopted.records();
        let definitions = self.definitions();
        let contract = self
            .path_contract(declaration, types)
            .map_err(|error| RuntimeError::typed_path_validation(error.to_string()))?;
        let root_type = self
            .type_declarations
            .get(&declaration.root)
            .copied()
            .ok_or_else(|| RuntimeError::typed_path_validation("missing host path root"))?;
        let result_type = self.portable_path_type(&contract.result, types)?;
        self.register_path_descriptor(
            HostPathDescriptorRegistration {
                root_type,
                result_type,
                definitions,
                segments: declaration.segments.clone(),
                access: declaration.access,
                schema_epoch: HostSchemaEpoch(declaration.schema_epoch),
            },
            types,
        )
    }

    pub(crate) fn resolve_path(
        &self,
        declaration: &DefinitionMetadata<HostPathDeclaration>,
    ) -> Result<HostPathDescriptorId, RuntimeError> {
        let declaration = declaration
            .import_into(&self.definitions, &CancellationToken::default())
            .map_err(|error| RuntimeError::typed_path_validation(error.to_string()))?;
        let mut matches = self.path_descriptors.values().filter(|descriptor| {
            descriptor.declaration == *declaration.records()
                && self.path_adapters.contains_key(&descriptor.id)
        });
        let descriptor = matches
            .next()
            .ok_or_else(|| RuntimeError::typed_path_validation("manual path is not installed"))?;
        if matches.next().is_some() {
            return Err(RuntimeError::typed_path_validation(
                "manual path has ambiguous bindings",
            ));
        }
        Ok(descriptor.id)
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
        gc: &GcHeap,
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
        validate_dynamic_arguments(gc, descriptor, &dynamic_args)?;
        Ok(HostPathViewHandle::new(
            root,
            None,
            descriptor,
            dynamic_args,
        ))
    }

    pub fn make_path_view_from_value(
        &self,
        gc: &GcHeap,
        root_or_view: &Value,
        descriptor_id: HostPathDescriptorId,
        dynamic_args: Vec<Value>,
    ) -> Result<HostPathViewHandle, RuntimeError> {
        let context = self.resolve_path_context(
            gc,
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

    pub fn dirty_paths(&self) -> Vec<HostPathMutationRecord> {
        self.dirty_paths.clone()
    }

    pub fn clear_dirty_paths(&mut self) {
        self.dirty_paths.clear();
    }

    pub(super) fn resolve_path_context(
        &self,
        gc: &GcHeap,
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
        validate_dynamic_arguments(gc, &descriptor, &dynamic_args)?;
        let (root, base_view) = match root_or_view {
            Value::HostRoot(id) => {
                let root = gc.host_root(*id).ok_or_else(|| {
                    RuntimeError::typed_path_validation("invalid host root descriptor")
                })?;
                let registered_root = self.roots.get(&root.object_id).ok_or_else(|| {
                    RuntimeError::typed_path_validation("host root is not registered")
                })?;
                if registered_root != &root {
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
                (root, None)
            }
            Value::HostPathView(id) => {
                let view = gc.host_path(*id).ok_or_else(|| {
                    RuntimeError::typed_path_validation("invalid host path descriptor")
                })?;
                self.validate_view(gc, &view)?;
                if operation.writes() && view.access != PathAccess::ReadWrite {
                    return Err(RuntimeError::typed_path_validation(
                        "host path view is not writable",
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
}
