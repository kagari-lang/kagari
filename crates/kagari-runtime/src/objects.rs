use crate::{
    RootedInterfaceMethod, Runtime,
    closure::{ClosureTarget, ClosureValueSnapshot},
    error::{RuntimeError, RuntimeErrorKind},
    execution_metadata::{MetadataEdge, interfaces::InterfaceSnapshotId},
    frame::{
        arguments::FrameArguments,
        types::{
            TypeEnvironment, arguments::TypeArgument, construction::ConstructionScope,
            operations::OperationBindings,
        },
    },
    gc::{HeapObjectId, interfaces::InterfaceValueSnapshot, reference::GcRef},
    module::{self, ExecutableModule},
    objects::invocation::MethodInvocation,
    value,
    value::{EnumTag, Value},
};
use kagari_abi::representation::ValueType;
use kagari_common::identity::table::DefinitionId;
use kagari_contract::{ids::FunctionRef, operations::IterOp};
use kagari_types::{
    language::binding,
    ty::{NominalTy, Ty},
};
use std::{cell::Ref, slice};
mod adapters;
mod application;
mod calls;
mod constructions;
mod interfaces;
pub(crate) mod invocation;
pub(crate) mod method;
mod method_view;
mod operations;

impl Runtime {
    /// Access backend execution transport through its original semantic owner.
    ///
    /// # Safety
    /// Keep every managed input in this runtime's roots or active traced storage
    /// until publication. Protect extracted results before collection, callbacks
    /// or mutation can remove their last owner. Returned loans must end before
    /// reentry; scoped host resources also require their original host lifetime.
    /// This entry preserves validation but cannot revive expired direct references.
    #[doc(hidden)]
    pub unsafe fn alloc_array_transport(
        &self,
        owner: &ExecutableModule,
        element: Ty,
        elements: Vec<Value>,
    ) -> Result<HeapObjectId, RuntimeError> {
        self.alloc_array(owner, element, elements)
    }

    pub(crate) fn alloc_array(
        &self,
        owner: &ExecutableModule,
        element: Ty,
        elements: Vec<Value>,
    ) -> Result<HeapObjectId, RuntimeError> {
        self.validate_executable_module(owner)?;
        self.validate_heap_payloads(&elements)?;
        self.gc.alloc_array(owner, element, elements)
    }

    /// Access backend execution transport through its original semantic owner.
    ///
    /// # Safety
    /// Keep every managed input in this runtime's roots or active traced storage
    /// until publication. Protect extracted results before collection, callbacks
    /// or mutation can remove their last owner. Returned loans must end before
    /// reentry; scoped host resources also require their original host lifetime.
    /// This entry preserves validation but cannot revive expired direct references.
    #[doc(hidden)]
    pub unsafe fn alloc_array_repeat_transport(
        &self,
        owner: &ExecutableModule,
        element: Ty,
        value: Value,
        count: usize,
    ) -> Result<HeapObjectId, RuntimeError> {
        self.alloc_array_repeat(owner, element, value, count)
    }

    pub(crate) fn alloc_array_repeat(
        &self,
        owner: &ExecutableModule,
        element: Ty,
        value: Value,
        count: usize,
    ) -> Result<HeapObjectId, RuntimeError> {
        self.validate_executable_module(owner)?;
        self.validate_heap_payloads(slice::from_ref(&value))?;
        self.gc.alloc_array_repeat(owner, element, value, count)
    }

    /// Access backend execution transport through its original semantic owner.
    ///
    /// # Safety
    /// Keep every managed input in this runtime's roots or active traced storage
    /// until publication. Protect extracted results before collection, callbacks
    /// or mutation can remove their last owner. Returned loans must end before
    /// reentry; scoped host resources also require their original host lifetime.
    /// This entry preserves validation but cannot revive expired direct references.
    #[doc(hidden)]
    pub unsafe fn alloc_struct_transport(
        &self,
        layout: module::StructLayoutRef,
        fields: Vec<Value>,
    ) -> Result<GcRef, RuntimeError> {
        self.alloc_struct(layout, fields)
    }

    pub(crate) fn alloc_struct(
        &self,
        layout: module::StructLayoutRef,
        fields: Vec<Value>,
    ) -> Result<GcRef, RuntimeError> {
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

    /// Access backend execution transport through its original semantic owner.
    ///
    /// # Safety
    /// Keep every managed input in this runtime's roots or active traced storage
    /// until publication. Protect extracted results before collection, callbacks
    /// or mutation can remove their last owner. Returned loans must end before
    /// reentry; scoped host resources also require their original host lifetime.
    /// This entry preserves validation but cannot revive expired direct references.
    #[doc(hidden)]
    pub unsafe fn alloc_enum_transport(
        &self,
        tag: value::EnumTag,
        fields: Vec<Value>,
    ) -> Result<HeapObjectId, RuntimeError> {
        self.alloc_enum(tag, fields)
    }

    pub(crate) fn alloc_enum(
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
    /// Access backend execution transport through its original semantic owner.
    ///
    /// # Safety
    /// Keep every managed input in this runtime's roots or active traced storage
    /// until publication. Protect extracted results before collection, callbacks
    /// or mutation can remove their last owner. Returned loans must end before
    /// reentry; scoped host resources also require their original host lifetime.
    /// This entry preserves validation but cannot revive expired direct references.
    #[doc(hidden)]
    pub unsafe fn make_interface_transport(
        &self,
        implementation: &ExecutableModule,
        table_index: usize,
        data: value::Value,
    ) -> Result<value::Value, RuntimeError> {
        self.make_interface(implementation, table_index, data)
    }

    pub(crate) fn make_interface(
        &self,
        implementation: &ExecutableModule,
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
        self.make_interface_applied(implementation, table_index, &arguments, None, data)
    }

    /// Instantiate a verified table using the allocating frame's type arguments
    /// and retained construction applications. A shared caller supplies its environment.
    /// Access backend execution transport through its original semantic owner.
    ///
    /// # Safety
    /// Keep every managed input in this runtime's roots or active traced storage
    /// until publication. Protect extracted results before collection, callbacks
    /// or mutation can remove their last owner. Returned loans must end before
    /// reentry; scoped host resources also require their original host lifetime.
    /// This entry preserves validation but cannot revive expired direct references.
    #[doc(hidden)]
    pub unsafe fn make_interface_applied_transport(
        &self,
        implementation: &ExecutableModule,
        table_index: usize,
        arguments: &[TypeArgument],
        construction_environment: Option<&TypeEnvironment>,
        data: Value,
    ) -> Result<Value, RuntimeError> {
        self.make_interface_applied(
            implementation,
            table_index,
            arguments,
            construction_environment,
            data,
        )
    }

    pub(crate) fn make_interface_applied(
        &self,
        implementation: &ExecutableModule,
        table_index: usize,
        arguments: &[TypeArgument],
        construction_environment: Option<&TypeEnvironment>,
        data: Value,
    ) -> Result<Value, RuntimeError> {
        let snapshot = self.prepare_interface_snapshot(
            implementation,
            table_index,
            arguments,
            construction_environment,
            data,
            false,
        )?;
        self.publish_interface(snapshot)
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
            &snapshot.description.concrete_expression,
            &snapshot.description.implementation,
            snapshot
                .description
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
        target: &NominalTy,
    ) -> Result<InterfaceSnapshotId, RuntimeError> {
        let invalid =
            || RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid interface upcast");
        let snapshot = self.gc.interface_metadata(id).ok_or_else(invalid)?;
        let slot = snapshot
            .description
            .parents
            .iter()
            .position(|parent| parent.interface == *target)
            .ok_or_else(invalid)?;
        let parent = &snapshot.description.parents[slot];
        if let Some(prepared) = snapshot.parent_views[slot].get() {
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
            binding.environment.as_ref(),
            data,
            use_view,
        )?;
        self.validate_interface_receiver(&prepared)?;
        self.validate_metadata(MetadataEdge::InterfaceView(&prepared))?;
        let prepared = self.gc.alloc_interface_snapshot(prepared)?;
        self.cache_parent_interface(id, slot, prepared)?;
        Ok(prepared)
    }

    /// Access backend execution transport through its original semantic owner.
    ///
    /// # Safety
    /// Keep every managed input in this runtime's roots or active traced storage
    /// until publication. Protect extracted results before collection, callbacks
    /// or mutation can remove their last owner. Returned loans must end before
    /// reentry; scoped host resources also require their original host lifetime.
    /// This entry preserves validation but cannot revive expired direct references.
    #[doc(hidden)]
    pub unsafe fn make_closure_transport(
        &self,
        implementation: &ExecutableModule,
        function: FunctionRef,
        captures: Vec<value::Value>,
        environment: Option<TypeEnvironment>,
    ) -> Result<value::Value, RuntimeError> {
        self.make_closure(implementation, function, captures, environment)
    }

    pub(crate) fn make_closure(
        &self,
        implementation: &ExecutableModule,
        function: FunctionRef,
        captures: Vec<value::Value>,
        environment: Option<TypeEnvironment>,
    ) -> Result<value::Value, RuntimeError> {
        let mut snapshot = self.prepare_closure(implementation, function, captures, environment)?;
        snapshot.bind_named(self)?;
        self.validate_metadata(MetadataEdge::Closure(&snapshot))?;
        self.gc
            .alloc_closure(self.modules(), snapshot)
            .map(Value::Closure)
    }

    /// Cold computation creation has already selected its resume body and values.
    pub(crate) fn make_retained_closure(
        &self,
        implementation: &ExecutableModule,
        function: FunctionRef,
        captures: Vec<Value>,
        environment: Option<TypeEnvironment>,
    ) -> Result<Value, RuntimeError> {
        let snapshot = self.prepare_closure(implementation, function, captures, environment)?;
        self.validate_metadata(MetadataEdge::Closure(&snapshot))?;
        self.gc
            .alloc_closure(self.modules(), snapshot)
            .map(Value::Closure)
    }

    fn prepare_closure(
        &self,
        implementation: &ExecutableModule,
        function: FunctionRef,
        captures: Vec<Value>,
        environment: Option<TypeEnvironment>,
    ) -> Result<ClosureValueSnapshot, RuntimeError> {
        self.validate_executable_module(implementation)?;
        if implementation.is_static_initializer(function) {
            return Err(RuntimeError::module_validation(
                "static initializer is not a callable value",
            ));
        }
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
        Ok(ClosureValueSnapshot {
            environment,
            implementation: implementation.clone(),
            target: ClosureTarget::Script(function),
            captures,
        })
    }

    /// Access backend execution transport through its original semantic owner.
    ///
    /// # Safety
    /// Keep every managed input in this runtime's roots or active traced storage
    /// until publication. Protect extracted results before collection, callbacks
    /// or mutation can remove their last owner. Returned loans must end before
    /// reentry; scoped host resources also require their original host lifetime.
    /// This entry preserves validation but cannot revive expired direct references.
    #[doc(hidden)]
    pub unsafe fn iter_operation_transport(
        &self,
        owner: &ExecutableModule,
        value: &value::Value,
        ty: &Ty,
        op: IterOp,
    ) -> Result<value::Value, RuntimeError> {
        self.iter_operation(owner, value, ty, op)
    }

    pub(crate) fn iter_operation(
        &self,
        owner: &ExecutableModule,
        value: &value::Value,
        ty: &Ty,
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
        owner: &ExecutableModule,
        value: &Value,
        ty: &TypeArgument,
        op: IterOp,
    ) -> Result<Value, RuntimeError> {
        self.validate_executable_module(owner)?;
        ty.validate(self)?;
        match op {
            IterOp::String(kind) => self.gc.new_string_iter(value, ty.ty(), kind, owner),
            IterOp::New => {
                let item = ty.derive(self, owner, |ty| match ty {
                    Ty::Range(item, _) | Ty::Array(item) => Some((**item).clone()),
                    Ty::Builtin(_) => Some(ty.clone()),
                    _ => None,
                })?;
                self.gc.new_iter(value, ty, item, owner)
            }
            IterOp::Next => {
                self.check_iterator_contract(owner, value, ty)?;
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
            }
        }
    }

    pub(crate) fn check_iterator_contract(
        &self,
        owner: &ExecutableModule,
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

    /// Access backend execution transport through its original semantic owner.
    ///
    /// # Safety
    /// Keep every managed input in this runtime's roots or active traced storage
    /// until publication. Protect extracted results before collection, callbacks
    /// or mutation can remove their last owner. Returned loans must end before
    /// reentry; scoped host resources also require their original host lifetime.
    /// This entry preserves validation but cannot revive expired direct references.
    #[doc(hidden)]
    pub unsafe fn make_capture_cell_transport(
        &self,
        ty: ValueType,
        value: value::Value,
    ) -> Result<value::Value, RuntimeError> {
        self.make_capture_cell(ty, value)
    }

    pub(crate) fn make_capture_cell(
        &self,
        ty: ValueType,
        value: value::Value,
    ) -> Result<value::Value, RuntimeError> {
        self.gc.alloc_cell(ty, value).map(Value::Cell)
    }

    /// Access backend execution transport through its original semantic owner.
    ///
    /// # Safety
    /// Keep every managed input in this runtime's roots or active traced storage
    /// until publication. Protect extracted results before collection, callbacks
    /// or mutation can remove their last owner. Returned loans must end before
    /// reentry; scoped host resources also require their original host lifetime.
    /// This entry preserves validation but cannot revive expired direct references.
    #[doc(hidden)]
    pub unsafe fn read_capture_cell_transport(
        &self,
        cell: &value::Value,
        ty: ValueType,
    ) -> Result<value::Value, RuntimeError> {
        self.read_capture_cell(cell, ty)
    }

    pub(crate) fn read_capture_cell(
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

    /// Access backend execution transport through its original semantic owner.
    ///
    /// # Safety
    /// Keep every managed input in this runtime's roots or active traced storage
    /// until publication. Protect extracted results before collection, callbacks
    /// or mutation can remove their last owner. Returned loans must end before
    /// reentry; scoped host resources also require their original host lifetime.
    /// This entry preserves validation but cannot revive expired direct references.
    #[doc(hidden)]
    pub unsafe fn write_capture_cell_transport(
        &self,
        cell: &value::Value,
        ty: ValueType,
        value: value::Value,
    ) -> Result<(), RuntimeError> {
        self.write_capture_cell(cell, ty, value)
    }

    pub(crate) fn write_capture_cell(
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
    /// A native payload loan rejects this escaping directory view.
    /// Access backend execution transport through its original semantic owner.
    ///
    /// # Safety
    /// Keep every managed input in this runtime's roots or active traced storage
    /// until publication. Protect extracted results before collection, callbacks
    /// or mutation can remove their last owner. Returned loans must end before
    /// reentry; scoped host resources also require their original host lifetime.
    /// This entry preserves validation but cannot revive expired direct references.
    #[doc(hidden)]
    pub unsafe fn resolve_closure_transport(
        &self,
        value: &value::Value,
    ) -> Result<Ref<'_, ClosureValueSnapshot>, RuntimeError> {
        self.resolve_closure(value)
    }

    pub(crate) fn resolve_closure(
        &self,
        value: &value::Value,
    ) -> Result<Ref<'_, ClosureValueSnapshot>, RuntimeError> {
        self.gc.ensure_no_storage_borrow()?;
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

    /// Resolve a method ID in the interface value's retained execution generation.
    /// Reject foreign identities, IDs beyond that generation's prefix and methods
    /// absent from its linked interface/parent tables. The result roots the receiver.
    /// Access backend execution transport through its original semantic owner.
    ///
    /// # Safety
    /// Keep every managed input in this runtime's roots or active traced storage
    /// until publication. Protect extracted results before collection, callbacks
    /// or mutation can remove their last owner. Returned loans must end before
    /// reentry; scoped host resources also require their original host lifetime.
    /// This entry preserves validation but cannot revive expired direct references.
    #[doc(hidden)]
    pub unsafe fn resolve_interface_method_transport(
        &self,
        value: &Value,
        method: &DefinitionId,
    ) -> Result<RootedInterfaceMethod, RuntimeError> {
        self.resolve_interface_method(value, method)
    }

    pub(crate) fn resolve_interface_method(
        &self,
        value: &Value,
        method: &DefinitionId,
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
        let method = *method;
        snapshot
            .description
            .receiver_table
            .owner
            .definitions()
            .resolve(method)
            .map_err(|error| RuntimeError::module_validation(error.to_string()))?;
        let root = self.root_value(*value).ok_or_else(|| {
            RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid interface handle")
        })?;
        let slot = snapshot.description.methods.iter().position(|binding| {
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
                ConstructionScope::default(),
            );
        }
        let owner = snapshot
            .description
            .receiver_table
            .owner
            .definitions()
            .parent(method)
            .map_err(|error| RuntimeError::module_validation(error.to_string()))?
            .ok_or_else(|| RuntimeError::module_validation("interface method parent"))?;
        let parent = snapshot
            .description
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
            let slot = snapshot.description.methods.iter().position(|binding| {
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
                    ConstructionScope::default(),
                );
            }
        }
        Err(RuntimeError::module_validation(
            "interface method unavailable",
        ))
    }

    /// Creates a parent interface view using already compiled tables from the
    /// receiver's retained execution version. It never specializes at runtime.
    /// Access backend execution transport through its original semantic owner.
    ///
    /// # Safety
    /// Keep every managed input in this runtime's roots or active traced storage
    /// until publication. Protect extracted results before collection, callbacks
    /// or mutation can remove their last owner. Returned loans must end before
    /// reentry; scoped host resources also require their original host lifetime.
    /// This entry preserves validation but cannot revive expired direct references.
    #[doc(hidden)]
    pub unsafe fn upcast_interface_transport(
        &self,
        value: &value::Value,
        source: &NominalTy,
        target: &NominalTy,
    ) -> Result<value::Value, RuntimeError> {
        self.upcast_interface(value, source, target)
    }

    pub(crate) fn upcast_interface(
        &self,
        value: &value::Value,
        source: &NominalTy,
        target: &NominalTy,
    ) -> Result<value::Value, RuntimeError> {
        let invalid =
            || RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid interface upcast");
        let Value::Interface(id) = value else {
            return Err(invalid());
        };
        let _root = self.root_value(*value).ok_or_else(invalid)?;
        let snapshot = self.gc.interface_snapshot(*id).ok_or_else(invalid)?;
        if snapshot.description.interface_type() != source {
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
    /// Access backend execution transport through its original semantic owner.
    ///
    /// # Safety
    /// Keep every managed input in this runtime's roots or active traced storage
    /// until publication. Protect extracted results before collection, callbacks
    /// or mutation can remove their last owner. Returned loans must end before
    /// reentry; scoped host resources also require their original host lifetime.
    /// This entry preserves validation but cannot revive expired direct references.
    #[doc(hidden)]
    pub unsafe fn resolve_interface_method_slot_transport(
        &self,
        value: &Value,
        interface: &NominalTy,
        slot: usize,
        arguments: &[TypeArgument],
    ) -> Result<RootedInterfaceMethod, RuntimeError> {
        self.resolve_interface_method_slot(value, interface, slot, arguments)
    }

    pub(crate) fn resolve_interface_method_slot(
        &self,
        value: &Value,
        interface: &NominalTy,
        slot: usize,
        arguments: &[TypeArgument],
    ) -> Result<RootedInterfaceMethod, RuntimeError> {
        self.prepare_interface_method_slot(
            value,
            interface,
            slot,
            arguments,
            OperationBindings::default(),
            ConstructionScope::default(),
        )
    }

    pub(crate) fn prepare_interface_method_slot(
        &self,
        value: &Value,
        interface: &NominalTy,
        slot: usize,
        arguments: &[TypeArgument],
        operations: OperationBindings,
        constructions: ConstructionScope,
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
            constructions,
        )
    }

    fn select_interface_snapshot(
        &self,
        value: &Value,
        interface: &NominalTy,
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
        let same = snapshot.description.interface_type() == interface;
        drop(snapshot);
        if same {
            Ok(id)
        } else {
            self.parent_snapshot(id, interface)
        }
    }

    /// Access backend execution transport through its original semantic owner.
    ///
    /// # Safety
    /// Keep every managed input in this runtime's roots or active traced storage
    /// until publication. Protect extracted results before collection, callbacks
    /// or mutation can remove their last owner. Returned loans must end before
    /// reentry; scoped host resources also require their original host lifetime.
    /// This entry preserves validation but cannot revive expired direct references.
    #[doc(hidden)]
    pub unsafe fn validate_interface_method_arguments_transport(
        &self,
        method: &RootedInterfaceMethod,
        arguments: &[value::Value],
    ) -> Result<(), RuntimeError> {
        self.validate_interface_method_arguments(method, arguments)
    }

    pub(crate) fn validate_interface_method_arguments(
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
        let adapter = {
            let view = method.view(self)?;
            view.validate_result(self, &result)?;
            view.result_adapter().cloned()
        };
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
            self.make_interface_applied(
                &adapter.owner,
                adapter.table,
                &arguments,
                adapter.environment.as_ref(),
                result,
            )
        } else {
            Ok(result)
        }
    }

    /// Access backend execution transport through its original semantic owner.
    ///
    /// # Safety
    /// Keep every managed input in this runtime's roots or active traced storage
    /// until publication. Protect extracted results before collection, callbacks
    /// or mutation can remove their last owner. Returned loans must end before
    /// reentry; scoped host resources also require their original host lifetime.
    /// This entry preserves validation but cannot revive expired direct references.
    #[doc(hidden)]
    pub unsafe fn validate_interface_method_result_transport(
        &self,
        method: &RootedInterfaceMethod,
        result: &value::Value,
    ) -> Result<(), RuntimeError> {
        self.validate_interface_method_result(method, result)
    }

    pub(crate) fn validate_interface_method_result(
        &self,
        method: &RootedInterfaceMethod,
        result: &value::Value,
    ) -> Result<(), RuntimeError> {
        method.view(self)?.validate_result(self, result)
    }
}
