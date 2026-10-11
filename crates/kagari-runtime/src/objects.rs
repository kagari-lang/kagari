use crate::{
    RootedInterfaceMethod, Runtime,
    closure::{ClosureTarget, ClosureValueSnapshot},
    error::{RuntimeError, RuntimeErrorKind},
    execution_metadata::{MetadataEdge, interfaces::InterfaceSnapshotId, links::MetadataCache},
    frame::{
        arguments::FrameArguments,
        types::{
            EnvironmentRecord, TypeEnvironment, arguments::TypeArgument,
            operations::OperationBindings,
        },
    },
    gc::{
        HeapObjectId,
        interfaces::{
            InterfaceMethodBinding, InterfaceParentBinding, InterfaceResultBinding,
            InterfaceValueSnapshot,
        },
    },
    module::{self, LoadedModule},
    objects::invocation::MethodInvocation,
    value,
    value::{EnumTag, Value},
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::module::CallableTarget;
use kagari_common::identity::{
    DefinitionKind, reference::DefinitionReference, table::DefinitionId,
};
use kagari_contract::{ids::FunctionRef, operations::IterOp, types as abi, types::PublicItem};
use kagari_types::{
    declaration::native::NativeStorageLayout,
    language::binding,
    ty::{NominalTy, Ty, substitution::TypeSubstitution},
};
use std::{cell::Ref, slice};
mod application;
mod calls;
pub(crate) mod invocation;
pub(crate) mod method;
mod method_view;
mod operations;

impl Runtime {
    pub fn alloc_array(
        &self,
        owner: &LoadedModule,
        element: Ty<DefinitionId>,
        elements: Vec<Value>,
    ) -> Result<HeapObjectId, RuntimeError> {
        self.validate_loaded_module(owner)?;
        self.validate_heap_payloads(&elements)?;
        self.gc.alloc_array(owner, element, elements)
    }

    pub fn alloc_array_repeat(
        &self,
        owner: &LoadedModule,
        element: Ty<DefinitionId>,
        value: Value,
        count: usize,
    ) -> Result<HeapObjectId, RuntimeError> {
        self.validate_loaded_module(owner)?;
        self.validate_heap_payloads(slice::from_ref(&value))?;
        self.gc.alloc_array_repeat(owner, element, value, count)
    }

    pub fn alloc_struct(
        &self,
        layout: module::StructLayoutRef,
        fields: Vec<Value>,
    ) -> Result<HeapObjectId, RuntimeError> {
        self.validate_heap_payloads(&fields)?;
        // The layout retains a verified generation; allocation does not depend on
        // whether that generation remains a current module-store entry.
        if !layout.module().belongs_to(self.host.owner()) {
            return Err(RuntimeError::module_validation(
                "struct layout belongs to a different runtime",
            ));
        }
        self.gc.alloc_struct(layout, fields)
    }

    pub fn alloc_enum(
        &self,
        tag: value::EnumTag,
        fields: Vec<Value>,
    ) -> Result<HeapObjectId, RuntimeError> {
        self.validate_heap_payloads(&fields)?;
        if let EnumTag::Declared(layout) = &tag
            && !layout.module().belongs_to(self.host.owner())
        {
            return Err(RuntimeError::module_validation(
                "enum layout belongs to a different runtime",
            ));
        }
        self.gc.alloc_enum(tag, fields)
    }

    /// Box a concrete script value or durable host root behind a verified table.
    /// The resulting heap object retains the table's entire execution version.
    pub fn make_interface(
        &self,
        implementation: &LoadedModule,
        table_index: usize,
        data: value::Value,
    ) -> Result<value::Value, RuntimeError> {
        let arguments = implementation
            .bytecode
            .interface_tables
            .get(table_index)
            .ok_or_else(|| RuntimeError::module_validation("invalid interface table"))?
            .arguments
            .clone();
        let arguments = self.resolve_type_arguments(implementation, &arguments)?;
        self.make_interface_applied(implementation, table_index, &arguments, data)
    }

    /// Instantiate a verified table using the allocating frame's type arguments.
    pub fn make_interface_applied(
        &self,
        implementation: &LoadedModule,
        table_index: usize,
        arguments: &[TypeArgument],
        data: Value,
    ) -> Result<Value, RuntimeError> {
        self.make_interface_view(implementation, table_index, arguments, data, false)
    }

    fn make_interface_view(
        &self,
        implementation: &LoadedModule,
        table_index: usize,
        arguments: &[TypeArgument],
        data: value::Value,
        use_view: bool,
    ) -> Result<value::Value, RuntimeError> {
        let snapshot = self.prepare_interface_snapshot(
            implementation,
            table_index,
            arguments,
            data,
            use_view,
        )?;
        self.publish_interface(snapshot)
    }

    fn prepare_interface_snapshot(
        &self,
        implementation: &LoadedModule,
        table_index: usize,
        arguments: &[TypeArgument],
        data: Value,
        use_view: bool,
    ) -> Result<InterfaceValueSnapshot, RuntimeError> {
        let invalid = || RuntimeError::module_validation("invalid interface implementation table");
        if !implementation.belongs_to(self.host.owner()) {
            return Err(invalid());
        }
        for argument in arguments {
            argument.validate(self)?;
        }
        let concrete_arguments = arguments
            .iter()
            .map(|argument| argument.ty().clone())
            .collect::<Vec<_>>();
        let linked = implementation
            .bytecode
            .interface_tables
            .get(table_index)
            .ok_or_else(invalid)?;
        if linked.arguments.iter().all(Ty::is_concrete) && linked.arguments != concrete_arguments {
            return Err(invalid());
        }
        let template = implementation
            .bytecode
            .public_items
            .iter()
            .find_map(|item| match item {
                PublicItem::InterfaceTable(table) if table.declaration == linked.declaration => {
                    Some(table.as_ref())
                }
                _ => None,
            })
            .ok_or_else(invalid)?;
        let table = template
            .instantiate_scoped(&concrete_arguments, &[], Some(implementation.definitions()))
            .ok_or_else(invalid)?;
        let environment = if template.generic_params.is_empty() {
            None
        } else {
            Some(self.alloc_environment(EnvironmentRecord::new(
                self.definition_context(),
                template.generic_params.clone(),
                arguments.to_vec(),
            )?)?)
        };
        let mut substitution = TypeSubstitution::default();
        for (parameter, argument) in template.generic_params.iter().zip(&concrete_arguments) {
            substitution.bind(&parameter.owner, parameter.position, argument);
        }
        let concrete_type = table.for_type.clone();
        if !concrete_type.is_concrete() {
            return Err(invalid());
        }
        let Ty::Trait(interface_type) = &table.trait_type else {
            return Err(invalid());
        };
        if !table.trait_type.is_concrete() {
            return Err(invalid());
        }
        let view = if use_view {
            Some(linked.view.as_ref().ok_or_else(invalid)?)
        } else {
            None
        };
        let view_interface = view
            .map(|view| substitution.apply_nominal(&view.interface, &Default::default()))
            .transpose()
            .map_err(|_| invalid())?;
        let interface_type = view_interface.as_ref().unwrap_or(interface_type);
        let module = implementation
            .definition(interface_type.declaration)?
            .module();
        let trait_contract = implementation
            .members()
            .find(|member| &member.bytecode.identity == module)
            .and_then(|member| {
                abi::trait_contract_in(
                    Some(member.definitions()),
                    &member.bytecode.identity,
                    &member.bytecode.public_items,
                    &member.bytecode.trait_contracts,
                    &interface_type.declaration,
                )
                .cloned()
            })
            .ok_or_else(invalid)?;
        if table.methods.iter().any(|method| {
            !trait_contract
                .methods
                .iter()
                .any(|declared| declared.name == method.name)
        }) {
            return Err(invalid());
        }
        let mut methods = Vec::with_capacity(trait_contract.methods.len());
        for declared in &trait_contract.methods {
            let method = template
                .methods
                .iter()
                .find(|method| method.name == declared.name);
            let Some(method) = method else {
                return Err(invalid());
            };
            let method_id = implementation
                .definitions()
                .lookup_child(
                    interface_type.declaration,
                    DefinitionKind::Method,
                    &method.name,
                    0,
                )
                .ok_or_else(invalid)?;
            let mut candidates = linked
                .methods
                .iter()
                .filter(|slot| slot.method == method_id);
            let Some(slot) = candidates.next() else {
                return Err(invalid());
            };
            if candidates.next().is_some() {
                return Err(invalid());
            }
            let result_adapter = view
                .and_then(|view| {
                    view.results
                        .iter()
                        .find(|adapter| adapter.method == method_id)
                })
                .map(|adapter| {
                    calls::interface_binding(
                        implementation,
                        &adapter.implementation,
                        environment.clone(),
                    )
                })
                .transpose()?;
            methods.push(Some(InterfaceMethodBinding {
                identity: Default::default(),
                receiver_operations: MetadataCache::new(),
                parameters: method.generic_params.clone(),
                entry_parameters: match slot.target {
                    CallableTarget::Script(target) => implementation.bytecode.functions
                        [target.index()]
                    .metadata
                    .semantic
                    .generic
                    .as_ref(),
                    CallableTarget::Native(target) => implementation.bytecode.native_imports
                        [target.index()]
                    .generic
                    .as_ref(),
                }
                .map(|body| body.parameters.clone())
                .unwrap_or_default(),
                entry_arguments: slot.arguments.clone(),
                result_adapter,
                method: method_id,
                target: slot.target,
                parameter_types: method.params.iter().map(|param| param.ty.clone()).collect(),
                return_type: method.return_type.clone(),
            }));
        }
        Ok(InterfaceValueSnapshot {
            receiver_operations: MetadataCache::new(),
            receiver_table: InterfaceResultBinding {
                owner: implementation.clone(),
                table: table_index,
                arguments: template
                    .generic_params
                    .iter()
                    .map(|parameter| parameter.as_type())
                    .collect(),
                environment: environment.clone(),
            },
            parents: linked
                .parents
                .iter()
                .map(|parent| {
                    Ok(InterfaceParentBinding {
                        prepared: MetadataCache::new(),
                        interface: substitution
                            .apply_nominal(&parent.interface, &Default::default())
                            .map_err(|_| invalid())?,
                        binding: calls::interface_binding(
                            implementation,
                            &parent.implementation,
                            environment.clone(),
                        )?,
                        view: parent.view,
                    })
                })
                .collect::<Result<_, RuntimeError>>()?,
            data,
            concrete_type,
            concrete_expression: template.for_type.clone(),
            interface_type: interface_type.clone(),
            interface_expression: match view {
                Some(view) => view.interface.clone(),
                None => match &template.trait_type {
                    Ty::Trait(ty) => ty.clone(),
                    _ => return Err(invalid()),
                },
            },
            environment,
            implementation: implementation.clone(),
            methods,
        })
    }

    fn publish_interface(&self, snapshot: InterfaceValueSnapshot) -> Result<Value, RuntimeError> {
        self.validate_interface_receiver(&snapshot)?;
        self.validate_metadata(MetadataEdge::InterfaceView(&snapshot))?;
        let id = self.gc.alloc_interface_snapshot(snapshot)?;
        self.gc.alloc_interface(id).map(Value::Interface)
    }

    fn validate_interface_receiver(
        &self,
        snapshot: &InterfaceValueSnapshot,
    ) -> Result<(), RuntimeError> {
        if !matches!(snapshot.data, Value::HostRoot(_)) {
            self.validate_heap_payloads(slice::from_ref(&snapshot.data))?;
        }
        // Cached metadata does not validate mutable host ownership/schema or GC
        // handles. Check the receiver again whenever an escaping view is published.
        if !self.matches_type_in(
            &snapshot.data,
            &snapshot.concrete_expression,
            &snapshot.implementation,
            snapshot
                .environment
                .as_ref()
                .map(|environment| environment.types.as_ref()),
        ) {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "invalid interface receiver",
            ));
        }
        Ok(())
    }

    fn publish_interface_view(&self, id: InterfaceSnapshotId) -> Result<Value, RuntimeError> {
        let snapshot = self
            .gc
            .interface_metadata(id)
            .ok_or_else(|| RuntimeError::module_validation("invalid interface snapshot"))?;
        self.validate_interface_receiver(&snapshot)?;
        self.validate_metadata(MetadataEdge::Interface(id))?;
        drop(snapshot);
        self.gc.alloc_interface(id).map(Value::Interface)
    }

    fn parent_snapshot(
        &self,
        id: InterfaceSnapshotId,
        target: &NominalTy<DefinitionId>,
    ) -> Result<InterfaceSnapshotId, RuntimeError> {
        let invalid =
            || RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid interface upcast");
        let snapshot = self.gc.interface_metadata(id).ok_or_else(invalid)?;
        let slot = snapshot
            .parents
            .iter()
            .position(|parent| parent.interface == *target)
            .ok_or_else(invalid)?;
        let parent = &snapshot.parents[slot];
        if let Some(prepared) = parent.prepared.get() {
            let view = self.gc.interface_metadata(*prepared).ok_or_else(invalid)?;
            self.validate_interface_receiver(&view)?;
            return Ok(*prepared);
        }
        let binding = parent.binding.clone();
        let data = snapshot.data;
        let use_view = parent.view;
        drop(snapshot);
        let prepared = self.prepare_interface_snapshot(
            &binding.owner,
            binding.table,
            &self.type_arguments(
                &binding.owner,
                binding
                    .environment
                    .as_ref()
                    .map(|environment| environment.types.clone()),
                &binding.arguments,
            )?,
            data,
            use_view,
        )?;
        self.validate_interface_receiver(&prepared)?;
        self.validate_metadata(MetadataEdge::InterfaceView(&prepared))?;
        let prepared = self.gc.alloc_interface_snapshot(prepared)?;
        self.cache_parent_interface(id, slot, prepared)?;
        Ok(prepared)
    }

    pub fn make_closure(
        &self,
        implementation: &LoadedModule,
        function: FunctionRef,
        captures: Vec<value::Value>,
        environment: Option<TypeEnvironment>,
    ) -> Result<value::Value, RuntimeError> {
        self.validate_loaded_module(implementation)?;
        let metadata = implementation
            .bytecode
            .functions
            .get(function.index())
            .ok_or_else(|| RuntimeError::module_validation("invalid closure function"))?;
        if captures.len() > metadata.metadata.params.len()
            || !captures
                .iter()
                .zip(&metadata.metadata.params)
                .all(|(value, ty)| value.has_representation(*ty))
        {
            return Err(RuntimeError::module_validation(
                "closure capture contract mismatch",
            ));
        }
        if metadata
            .metadata
            .semantic
            .generic
            .as_ref()
            .is_some_and(|body| {
                environment
                    .as_ref()
                    .is_none_or(|environment| !environment.types.matches(body))
            })
        {
            return Err(RuntimeError::module_validation(
                "closure generic environment",
            ));
        }
        if let Some(environment) = &environment {
            for (index, value) in captures.iter().enumerate() {
                if let Some(ty) = metadata.metadata.semantic.params.get(&index)
                    && !self.matches_capture_type(value, ty, implementation, &environment.types)
                {
                    return Err(RuntimeError::module_validation(
                        "closure semantic capture mismatch",
                    ));
                }
            }
        }
        self.validate_heap_payloads(&captures)?;
        let snapshot = ClosureValueSnapshot {
            environment,
            implementation: implementation.clone(),
            target: ClosureTarget::Script(function),
            captures,
        };
        self.validate_metadata(MetadataEdge::Closure(&snapshot))?;
        self.gc.alloc_closure(snapshot).map(Value::Closure)
    }

    pub fn iter_operation(
        &self,
        owner: &LoadedModule,
        value: &value::Value,
        ty: &Ty<DefinitionId>,
        op: IterOp,
    ) -> Result<value::Value, RuntimeError> {
        let ty = self
            .resolve_type_arguments(owner, slice::from_ref(ty))?
            .pop()
            .ok_or_else(|| RuntimeError::module_validation("iterator type scope"))?;
        self.iter_operation_with_type(owner, value, &ty, op)
    }

    pub(crate) fn iter_operation_with_type(
        &self,
        owner: &LoadedModule,
        value: &Value,
        ty: &TypeArgument,
        op: IterOp,
    ) -> Result<Value, RuntimeError> {
        self.validate_loaded_module(owner)?;
        ty.validate(self)?;
        match op {
            IterOp::String(kind) => self.gc.new_string_iter(value, ty.ty(), kind, owner),
            IterOp::New => {
                let item = ty.derive(self, owner, |ty| match ty {
                    Ty::Range(item, _) | Ty::Array(item) | Ty::Set(item, _) => {
                        Some((**item).clone())
                    }
                    Ty::NativeObject(nominal) => self
                        .native_entries
                        .storage
                        .get_id(nominal.declaration)
                        .and_then(|storage| match storage.layout() {
                            NativeStorageLayout::Sequence { element } => {
                                nominal.arguments.get(element).cloned()
                            }
                            _ => None,
                        }),
                    Ty::Map { key, value, .. } => {
                        Some(Ty::Tuple(vec![(**key).clone(), (**value).clone()]))
                    }
                    Ty::Builtin(_) => Some(ty.clone()),
                    _ => None,
                })?;
                self.gc.new_iter(value, ty, item, owner)
            }
            _ => {
                self.check_iterator_contract(owner, value, ty)?;
                if op == IterOp::Next {
                    let declaration = self
                        .definition_context()
                        .intern(&binding::option_declaration())
                        .map_err(|error| RuntimeError::module_validation(error.to_string()))?;
                    let applied = ty.derive(self, owner, |ty| match ty {
                        Ty::Iter(item) => Some(Ty::Enum(NominalTy {
                            declaration,
                            arguments: vec![(**item).clone()],
                            associated_types: Default::default(),
                        })),
                        _ => None,
                    })?;
                    self.gc.advance_iter_with(value, ty.ty(), |payload| {
                        self.make_enum_member(
                            owner,
                            &applied,
                            if payload.is_some() { "Some" } else { "None" },
                            payload.into_iter().collect(),
                        )
                    })
                } else {
                    self.gc.advance_iter(value, ty.ty(), op)
                }
            }
        }
    }

    pub(crate) fn check_iterator_contract(
        &self,
        owner: &LoadedModule,
        value: &Value,
        ty: &TypeArgument,
    ) -> Result<(), RuntimeError> {
        if !ty.matches(self, value, owner) {
            return Err(RuntimeError::module_validation(
                "iterator differs from its checked item contract",
            ));
        }
        Ok(())
    }

    pub fn make_capture_cell(
        &self,
        ty: ValueType,
        value: value::Value,
    ) -> Result<value::Value, RuntimeError> {
        self.gc.alloc_cell(ty, value).map(Value::Cell)
    }

    pub fn read_capture_cell(
        &self,
        cell: &value::Value,
        ty: ValueType,
    ) -> Result<value::Value, RuntimeError> {
        let Value::Cell(id) = cell else {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "expected capture cell",
            ));
        };
        self.gc.cell_get(*id, ty)
    }

    pub fn write_capture_cell(
        &self,
        cell: &value::Value,
        ty: ValueType,
        value: value::Value,
    ) -> Result<(), RuntimeError> {
        let Value::Cell(id) = cell else {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "expected capture cell",
            ));
        };
        self.gc.cell_set(*id, ty, value)
    }

    /// Borrow the checked heap record for inspection. Release the view before
    /// heap mutation, collection or execution that may perform either operation.
    pub fn resolve_closure(
        &self,
        value: &value::Value,
    ) -> Result<Ref<'_, ClosureValueSnapshot>, RuntimeError> {
        let Value::Closure(id) = value else {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "expected closure",
            ));
        };
        self.gc.closure_snapshot(*id).ok_or_else(|| {
            RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid closure handle")
        })
    }

    pub fn resolve_interface_method<I: DefinitionReference>(
        &self,
        value: &Value,
        method: &I,
    ) -> Result<RootedInterfaceMethod, RuntimeError> {
        let Value::Interface(object) = value else {
            return Err(RuntimeError::module_validation("expected interface value"));
        };
        let id = self
            .gc
            .interface_snapshot_id(*object)
            .ok_or_else(|| RuntimeError::module_validation("invalid interface handle"))?;
        let snapshot = self
            .gc
            .interface_metadata(id)
            .ok_or_else(|| RuntimeError::module_validation("invalid interface handle"))?;
        let method = method
            .resolve(snapshot.receiver_table.owner.definitions())
            .map_err(|error| RuntimeError::module_validation(error.to_string()))?;
        let root = self.root_value(*value).ok_or_else(|| {
            RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid interface handle")
        })?;
        let slot = snapshot.methods.iter().position(|binding| {
            binding
                .as_ref()
                .is_some_and(|binding| binding.method == method)
        });
        if let Some(slot) = slot {
            drop(snapshot);
            return self.apply_interface_method(
                RootedInterfaceMethod::from_interface(self, root, id, slot)?,
                &[],
                OperationBindings::default(),
            );
        }
        let owner = snapshot
            .receiver_table
            .owner
            .definitions()
            .parent(method)
            .map_err(|error| RuntimeError::module_validation(error.to_string()))?
            .ok_or_else(|| RuntimeError::module_validation("interface method parent"))?;
        let parent = snapshot
            .parents
            .iter()
            .find(|parent| parent.interface.declaration == owner)
            .map(|parent| parent.interface.clone());
        drop(snapshot);
        if let Some(parent) = parent {
            let id = self.parent_snapshot(id, &parent)?;
            let snapshot = self.gc.interface_metadata(id).ok_or_else(|| {
                RuntimeError::module_validation("invalid parent interface snapshot")
            })?;
            let slot = snapshot.methods.iter().position(|binding| {
                binding
                    .as_ref()
                    .is_some_and(|binding| binding.method == method)
            });
            drop(snapshot);
            if let Some(slot) = slot {
                return self.apply_interface_method(
                    RootedInterfaceMethod::from_interface(self, root, id, slot)?,
                    &[],
                    OperationBindings::default(),
                );
            }
        }
        Err(RuntimeError::module_validation(
            "interface method unavailable",
        ))
    }

    /// Creates a parent interface view using already compiled tables from the
    /// receiver's retained execution version. It never specializes at runtime.
    pub fn upcast_interface(
        &self,
        value: &value::Value,
        source: &NominalTy<DefinitionId>,
        target: &NominalTy<DefinitionId>,
    ) -> Result<value::Value, RuntimeError> {
        let invalid =
            || RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid interface upcast");
        let Value::Interface(id) = value else {
            return Err(invalid());
        };
        let _root = self.root_value(*value).ok_or_else(invalid)?;
        let snapshot = self.gc.interface_snapshot(*id).ok_or_else(invalid)?;
        if snapshot.interface_type != *source {
            return Err(invalid());
        }
        if source == target {
            return Ok(*value);
        }
        drop(snapshot);
        let snapshot = self.gc.interface_snapshot_id(*id).ok_or_else(invalid)?;
        self.publish_interface_view(self.parent_snapshot(snapshot, target)?)
    }

    /// Resolves a trait declaration's verified method ordinal without a
    /// name or declaration search during script dispatch.
    pub fn resolve_interface_method_slot(
        &self,
        value: &Value,
        interface: &NominalTy<DefinitionId>,
        slot: usize,
        arguments: &[TypeArgument],
    ) -> Result<RootedInterfaceMethod, RuntimeError> {
        self.prepare_interface_method_slot(
            value,
            interface,
            slot,
            arguments,
            OperationBindings::default(),
        )
    }

    pub(crate) fn prepare_interface_method_slot(
        &self,
        value: &Value,
        interface: &NominalTy<DefinitionId>,
        slot: usize,
        arguments: &[TypeArgument],
        operations: OperationBindings,
    ) -> Result<RootedInterfaceMethod, RuntimeError> {
        if !matches!(value, Value::Interface(_)) {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "expected interface value",
            ));
        }
        let root = self.root_value(*value).ok_or_else(|| {
            RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid interface handle")
        })?;
        let id = self.select_interface_snapshot(value, interface)?;
        self.apply_interface_method(
            RootedInterfaceMethod::from_interface(self, root, id, slot)?,
            arguments,
            operations,
        )
    }

    fn select_interface_snapshot(
        &self,
        value: &Value,
        interface: &NominalTy<DefinitionId>,
    ) -> Result<InterfaceSnapshotId, RuntimeError> {
        let Value::Interface(id) = value else {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "expected interface value",
            ));
        };
        let id = self.gc.interface_snapshot_id(*id).ok_or_else(|| {
            RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid interface handle")
        })?;
        let snapshot = self
            .gc
            .interface_metadata(id)
            .ok_or_else(|| RuntimeError::module_validation("invalid interface snapshot"))?;
        let same = snapshot.interface_type == *interface;
        drop(snapshot);
        if same {
            Ok(id)
        } else {
            self.parent_snapshot(id, interface)
        }
    }

    pub fn validate_interface_method_arguments(
        &self,
        method: &RootedInterfaceMethod,
        arguments: &[value::Value],
    ) -> Result<(), RuntimeError> {
        method
            .view(self)?
            .validate_arguments(self, FrameArguments::plain(arguments))
    }

    pub(crate) fn finish_method_result(
        &self,
        method: &MethodInvocation,
        result: Value,
    ) -> Result<Value, RuntimeError> {
        method.view(self)?.validate_result(self, &result)?;
        let adapter = method.view(self)?.result_adapter().cloned();
        if let Some(adapter) = adapter {
            // Keep the raw return alive until its interface wrapper is published.
            let _root = self
                .root_value(result)
                .ok_or_else(|| RuntimeError::module_validation("invalid interface result root"))?;
            let arguments = self.type_arguments(
                &adapter.owner,
                adapter
                    .environment
                    .as_ref()
                    .map(|environment| environment.types.clone()),
                &adapter.arguments,
            )?;
            self.make_interface_applied(&adapter.owner, adapter.table, &arguments, result)
        } else {
            Ok(result)
        }
    }

    pub fn validate_interface_method_result(
        &self,
        method: &RootedInterfaceMethod,
        result: &value::Value,
    ) -> Result<(), RuntimeError> {
        method.view(self)?.validate_result(self, result)
    }
}
