mod application;
mod calls;
pub(crate) mod method;
mod operations;

use crate::{
    RootedInterfaceMethod, Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    frame::types::{TypeEnvironment, arguments::TypeArgument},
    gc::{
        self, HeapObjectId, RootedValue,
        interfaces::{
            InterfaceMethodBinding, InterfaceParentBinding, InterfaceResultBinding,
            InterfaceValueSnapshot,
        },
    },
    module::{self, LoadedModule},
    value::{self, EnumTag, Value},
};
use kagari_bytecode::module::CallableTarget;

use kagari_abi::{
    ids::FunctionRef,
    operations::IterOp,
    representation::ValueType,
    types::{self as abi, AbiType, NominalAbiType, PublicAbiItem, substitution::TypeSubstitution},
};
use kagari_common::identity::{DefinitionKind, DefinitionPath, DefinitionPathSegment};
use std::{cell::OnceCell, rc::Rc, slice};

impl Runtime {
    pub fn alloc_array(
        &self,
        owner: &LoadedModule,
        element: AbiType,
        elements: Vec<Value>,
    ) -> Result<HeapObjectId, RuntimeError> {
        self.validate_loaded_module(owner)?;
        self.validate_heap_payloads(&elements)?;
        self.gc.alloc_array(owner, element, elements)
    }

    pub fn alloc_array_repeat(
        &self,
        owner: &LoadedModule,
        element: AbiType,
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
    ) -> Result<Rc<InterfaceValueSnapshot>, RuntimeError> {
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
        if linked.arguments.iter().all(AbiType::is_concrete)
            && linked.arguments != concrete_arguments
        {
            return Err(invalid());
        }
        let template = implementation
            .bytecode
            .public_items
            .iter()
            .find_map(|item| match item {
                PublicAbiItem::InterfaceTable(table) if table.declaration == linked.declaration => {
                    Some(table.as_ref())
                }
                _ => None,
            })
            .ok_or_else(invalid)?;
        let table = template
            .instantiate(&concrete_arguments)
            .ok_or_else(invalid)?;
        let environment = if template.generic_params.is_empty() {
            None
        } else {
            Some(Rc::new(TypeEnvironment::new(
                template.generic_params.clone(),
                arguments.to_vec(),
            )?))
        };
        let mut substitution = TypeSubstitution::default();
        for (parameter, argument) in template.generic_params.iter().zip(&concrete_arguments) {
            substitution.bind(&parameter.owner, parameter.position, argument);
        }
        let concrete_type = table.for_type.clone();
        if !concrete_type.is_concrete() {
            return Err(invalid());
        }
        let AbiType::Trait(interface_type) = &table.trait_type else {
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
        let trait_contract = implementation
            .members()
            .find(|member| member.bytecode.identity == interface_type.declaration.module)
            .and_then(|member| {
                abi::trait_contract(
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
            let mut path = interface_type.declaration.path.clone();
            path.push(DefinitionPathSegment {
                kind: DefinitionKind::Method,
                name: method.name.clone(),
                occurrence: 0,
            });
            let method_id = DefinitionPath {
                module: interface_type.declaration.module.clone(),
                path,
            };
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
                application: OnceCell::new(),
                receiver_operations: OnceCell::new(),
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
        Ok(Rc::new(InterfaceValueSnapshot {
            receiver_operations: OnceCell::new(),
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
                        prepared: OnceCell::new(),
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
                    AbiType::Trait(ty) => ty.clone(),
                    _ => return Err(invalid()),
                },
            },
            environment,
            implementation: implementation.clone(),
            methods,
        }))
    }

    fn publish_interface(
        &self,
        snapshot: Rc<InterfaceValueSnapshot>,
    ) -> Result<Value, RuntimeError> {
        self.validate_interface_receiver(&snapshot)?;
        let retention = self
            .modules
            .retain_runtime_program(&snapshot.implementation)
            .ok_or_else(|| {
                RuntimeError::module_validation("invalid interface implementation table")
            })?;
        self.gc
            .alloc_interface(snapshot, retention)
            .map(Value::Interface)
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
            snapshot.environment.as_deref(),
        ) {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "invalid interface receiver",
            ));
        }
        Ok(())
    }

    fn parent_snapshot(
        &self,
        snapshot: &InterfaceValueSnapshot,
        target: &NominalAbiType,
    ) -> Result<Rc<InterfaceValueSnapshot>, RuntimeError> {
        let parent = snapshot
            .parents
            .iter()
            .find(|parent| parent.interface == *target)
            .ok_or_else(|| {
                RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid interface upcast")
            })?;
        if let Some(prepared) = parent.prepared.get() {
            self.validate_interface_receiver(prepared)?;
            return Ok(prepared.clone());
        }
        let binding = &parent.binding;
        let prepared = self.prepare_interface_snapshot(
            &binding.owner,
            binding.table,
            &self.type_arguments(
                &binding.owner,
                binding.environment.clone(),
                &binding.arguments,
            )?,
            snapshot.data.clone(),
            parent.view,
        )?;
        self.validate_interface_receiver(&prepared)?;
        parent
            .prepared
            .set(prepared.clone())
            .expect("parent interface prepared once");
        Ok(prepared)
    }

    pub fn make_closure(
        &self,
        implementation: &LoadedModule,
        function: FunctionRef,
        captures: Vec<value::Value>,
        environment: Option<Rc<TypeEnvironment>>,
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
                    .is_none_or(|environment| !environment.matches(body))
            })
        {
            return Err(RuntimeError::module_validation(
                "closure generic environment",
            ));
        }
        if let Some(environment) = &environment {
            for (index, value) in captures.iter().enumerate() {
                if let Some(ty) = metadata.metadata.semantic.params.get(&index)
                    && !self.matches_capture_type(value, ty, implementation, environment)
                {
                    return Err(RuntimeError::module_validation(
                        "closure semantic capture mismatch",
                    ));
                }
            }
        }
        self.validate_heap_payloads(&captures)?;
        let retention = self
            .modules
            .retain_runtime_program(implementation)
            .ok_or_else(|| RuntimeError::module_validation("closure version unavailable"))?;
        self.gc
            .alloc_closure(
                gc::ClosureValueSnapshot {
                    environment,
                    implementation: implementation.clone(),
                    function,
                    captures,
                },
                retention,
            )
            .map(Value::Closure)
    }

    pub fn iter_operation(
        &self,
        owner: &LoadedModule,
        value: &value::Value,
        ty: &AbiType,
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
                    AbiType::Range(item, _) | AbiType::Array(item, _) | AbiType::Set(item, _) => {
                        Some((**item).clone())
                    }
                    AbiType::Map { key, value, .. } => {
                        Some(AbiType::Tuple(vec![(**key).clone(), (**value).clone()]))
                    }
                    AbiType::Builtin(_) => Some(ty.clone()),
                    _ => None,
                })?;
                self.gc.new_iter(value, ty, item, owner)
            }
            _ => {
                if !ty.matches(self, value, owner) {
                    return Err(RuntimeError::module_validation(
                        "iterator differs from its checked item contract",
                    ));
                }
                self.gc.advance_iter(value, ty.ty(), op)
            }
        }
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

    pub fn resolve_closure(
        &self,
        value: &value::Value,
    ) -> Result<Rc<gc::ClosureValueSnapshot>, RuntimeError> {
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

    pub fn resolve_interface_method(
        &self,
        value: &Value,
        method: &DefinitionPath,
    ) -> Result<RootedInterfaceMethod, RuntimeError> {
        let Value::Interface(id) = value else {
            return Err(RuntimeError::module_validation("expected interface value"));
        };
        let snapshot = self
            .gc
            .interface_snapshot(*id)
            .ok_or_else(|| RuntimeError::module_validation("invalid interface handle"))?;
        let root = self.root_value(value.clone()).ok_or_else(|| {
            RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid interface handle")
        })?;
        let slot = snapshot.methods.iter().position(|binding| {
            binding
                .as_ref()
                .is_some_and(|binding| &binding.method == method)
        });
        if let Some(slot) = slot {
            return self.apply_interface_method(
                RootedInterfaceMethod::from_interface(root, snapshot, slot),
                &[],
            );
        }
        let mut owner = method.clone();
        owner.path.pop();
        if let Some(parent) = snapshot
            .parents
            .iter()
            .find(|parent| parent.interface.declaration == owner)
        {
            let snapshot = self.parent_snapshot(&snapshot, &parent.interface)?;
            if let Some(slot) = snapshot.methods.iter().position(|binding| {
                binding
                    .as_ref()
                    .is_some_and(|binding| &binding.method == method)
            }) {
                return self.apply_interface_method(
                    RootedInterfaceMethod::from_interface(root, snapshot, slot),
                    &[],
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
        source: &NominalAbiType,
        target: &NominalAbiType,
    ) -> Result<value::Value, RuntimeError> {
        let invalid =
            || RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid interface upcast");
        let Value::Interface(id) = value else {
            return Err(invalid());
        };
        let _root = self.root_value(value.clone()).ok_or_else(invalid)?;
        let snapshot = self.gc.interface_snapshot(*id).ok_or_else(invalid)?;
        if snapshot.interface_type != *source {
            return Err(invalid());
        }
        if source == target {
            return Ok(value.clone());
        }
        self.publish_interface(self.parent_snapshot(&snapshot, target)?)
    }

    /// Resolves a trait declaration's verified method ordinal without a
    /// name or declaration search during script dispatch.
    pub fn resolve_interface_method_slot(
        &self,
        value: &Value,
        interface: &NominalAbiType,
        slot: usize,
        arguments: &[TypeArgument],
    ) -> Result<RootedInterfaceMethod, RuntimeError> {
        let (root, snapshot) = self.rooted_interface_snapshot(value)?;
        let snapshot = if snapshot.interface_type == *interface {
            snapshot
        } else {
            self.parent_snapshot(&snapshot, interface)?
        };
        if snapshot
            .methods
            .get(slot)
            .and_then(Option::as_ref)
            .is_none()
        {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "interface method unavailable",
            ));
        }
        self.apply_interface_method(
            RootedInterfaceMethod::from_interface(root, snapshot, slot),
            arguments,
        )
    }

    fn rooted_interface_snapshot(
        &self,
        value: &Value,
    ) -> Result<(RootedValue, Rc<InterfaceValueSnapshot>), RuntimeError> {
        let invalid =
            || RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid interface handle");
        let Value::Interface(id) = value else {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "expected interface value",
            ));
        };
        let root = self.root_value(value.clone()).ok_or_else(invalid)?;
        let snapshot = self.gc.interface_snapshot(*id).ok_or_else(invalid)?;
        Ok((root, snapshot))
    }

    pub fn validate_interface_method_arguments(
        &self,
        method: &RootedInterfaceMethod,
        arguments: &[value::Value],
    ) -> Result<(), RuntimeError> {
        if !method.implementation().belongs_to(self.host.owner())
            || arguments.len() != method.parameter_types().len()
            || !arguments
                .iter()
                .enumerate()
                .all(|(index, value)| match method.scoped_signature() {
                    Some(signature) => {
                        signature.params[index].matches(self, value, method.implementation())
                    }
                    None => self.matches_interface_method_abi(
                        value,
                        &method.parameter_types()[index],
                        method.implementation(),
                    ),
                })
        {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "interface method argument does not match its linked signature",
            ));
        }
        Ok(())
    }

    pub(crate) fn finish_interface_method_result(
        &self,
        method: &RootedInterfaceMethod,
        result: Value,
    ) -> Result<Value, RuntimeError> {
        self.validate_interface_method_result(method, &result)?;
        if let Some(adapter) = method.result_adapter() {
            // Keep the raw return alive until its interface wrapper is published.
            let _root = self
                .root_value(result.clone())
                .ok_or_else(|| RuntimeError::module_validation("invalid interface result root"))?;
            let arguments = self.type_arguments(
                &adapter.owner,
                adapter.environment.clone(),
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
        if !method.implementation().belongs_to(self.host.owner())
            || !match method.scoped_signature() {
                Some(signature) => signature
                    .result
                    .matches(self, result, method.implementation()),
                None => self.matches_interface_method_abi(
                    result,
                    method.return_type(),
                    method.implementation(),
                ),
            }
        {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "interface method result does not match its linked signature",
            ));
        }
        Ok(())
    }

    pub(super) fn matches_interface_method_abi(
        &self,
        value: &value::Value,
        ty: &AbiType,
        implementation: &LoadedModule,
    ) -> bool {
        if !self.gc.validate_value(value) {
            return false;
        }
        match (value, ty) {
            (Value::Tuple(values), AbiType::Tuple(types)) => {
                values.len() == types.len()
                    && values.iter().zip(types).all(|(value, ty)| {
                        self.matches_interface_method_abi(value, ty, implementation)
                    })
            }
            (Value::HostRoot(root), AbiType::Host(id)) => {
                self.host.matches_root(*root)
                    && self
                        .host
                        .host_type_by_declaration(id)
                        .is_some_and(|host| host.type_id == root.type_id())
            }
            _ => self.gc.matches_abi(value, ty, implementation),
        }
    }
}
