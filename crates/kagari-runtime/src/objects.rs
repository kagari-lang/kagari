use crate::RootedInterfaceMethod;
use crate::Runtime;
use crate::error::RuntimeError;
use crate::error::RuntimeErrorKind;
use crate::gc;
use crate::gc::HeapObjectId;
use crate::module;
use crate::module::LoadedModule;
use crate::value;
use crate::value::EnumTag;
use crate::value::Value;
use kagari_abi::ids::FunctionRef;
use kagari_abi::operations::IterOp;
use kagari_abi::representation::ValueType;
use kagari_abi::standard::declarations::native_trait_default;
use kagari_abi::types as abi;
use kagari_abi::types::AbiType;
use kagari_abi::types::NominalAbiType;
use kagari_abi::types::PublicAbiItem;
use kagari_bytecode as bytecode;
use kagari_common::identity::DefinitionId;
use kagari_common::identity::DefinitionKind;
use kagari_common::identity::DefinitionPathSegment;
use std::slice;

impl Runtime {
    pub fn alloc_array(&self, elements: Vec<Value>) -> Result<HeapObjectId, RuntimeError> {
        self.validate_heap_payloads(&elements)?;
        self.gc.alloc_array(elements)
    }

    pub fn alloc_map(&self, entries: Vec<(Value, Value)>) -> Result<HeapObjectId, RuntimeError> {
        for (key, value) in &entries {
            self.validate_heap_payloads(slice::from_ref(key))?;
            self.validate_heap_payloads(slice::from_ref(value))?;
        }
        self.gc.alloc_map(entries)
    }

    pub fn alloc_set(&self, values: Vec<Value>) -> Result<HeapObjectId, RuntimeError> {
        self.validate_heap_payloads(&values)?;
        self.gc.alloc_set(values)
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
        let invalid = || RuntimeError::module_validation("invalid interface implementation table");
        if !implementation.belongs_to(self.host.owner()) {
            return Err(invalid());
        }
        let linked = implementation
            .bytecode
            .interface_tables
            .get(table_index)
            .ok_or_else(invalid)?;
        let table = implementation
            .bytecode
            .public_items
            .iter()
            .find_map(|item| match item {
                PublicAbiItem::InterfaceTable(table) if table.declaration == linked.declaration => {
                    table.instantiate(&linked.arguments)
                }
                _ => None,
            })
            .ok_or_else(invalid)?;
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
        let trait_name = interface_type
            .declaration
            .path
            .last()
            .map(|segment| segment.name.as_str())
            .ok_or_else(invalid)?;
        let trait_contract = implementation
            .members()
            .find(|member| member.bytecode.identity == interface_type.declaration.module)
            .and_then(|member| {
                member
                    .bytecode
                    .trait_contracts
                    .iter()
                    .find(|contract| contract.declaration == interface_type.declaration)
                    .map(|contract| contract.abi.clone())
                    .or_else(|| {
                        member
                            .bytecode
                            .public_items
                            .iter()
                            .find_map(|item| match item {
                                PublicAbiItem::Trait(trait_abi) if trait_abi.name == trait_name => {
                                    Some(trait_abi.clone())
                                }
                                _ => None,
                            })
                    })
            })
            .or_else(|| abi::standard_trait_contract(&interface_type.declaration).cloned())
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
            let method = table
                .methods
                .iter()
                .find(|method| method.name == declared.name);
            let Some(method) = method else {
                if native_trait_default(&interface_type.declaration, &declared.name) {
                    methods.push(None);
                    continue;
                }
                return Err(invalid());
            };
            if !method.generic_params.is_empty() {
                return Err(invalid());
            }
            let mut path = interface_type.declaration.path.clone();
            path.push(DefinitionPathSegment {
                kind: DefinitionKind::Method,
                name: method.name.clone(),
                occurrence: 0,
            });
            let method_id = DefinitionId {
                module: interface_type.declaration.module.clone(),
                path,
            };
            let mut candidates = linked.methods.iter().filter(|slot| {
                slot.method == method_id
                    && implementation
                        .bytecode
                        .functions
                        .get(slot.function.index())
                        .and_then(|function| function.identity.as_ref())
                        .is_some_and(|identity| identity.arguments == linked.arguments)
            });
            let Some(slot) = candidates.next() else {
                return Err(invalid());
            };
            if candidates.next().is_some() {
                return Err(invalid());
            }
            methods.push(Some(gc::InterfaceMethodBinding {
                method: method_id,
                function: slot.function,
                parameter_types: method.params.iter().map(|param| param.ty.clone()).collect(),
                return_type: method.return_type.clone(),
            }));
        }
        if !matches!(data, Value::HostRoot(_)) {
            self.validate_heap_payloads(slice::from_ref(&data))?;
        }
        if !self.matches_interface_method_abi(&data, &concrete_type, implementation) {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "invalid interface receiver",
            ));
        }
        let retention = self
            .modules
            .retain_runtime_program(implementation)
            .ok_or_else(invalid)?;
        self.gc
            .alloc_interface(
                gc::InterfaceValueSnapshot {
                    data,
                    concrete_type,
                    interface_type: interface_type.clone(),
                    implementation: implementation.clone(),
                    methods,
                },
                retention,
            )
            .map(Value::Interface)
    }

    pub fn make_closure(
        &self,
        implementation: &LoadedModule,
        function: FunctionRef,
        captures: Vec<value::Value>,
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
        self.validate_heap_payloads(&captures)?;
        let retention = self
            .modules
            .retain_runtime_program(implementation)
            .ok_or_else(|| RuntimeError::module_validation("closure version unavailable"))?;
        self.gc
            .alloc_closure(
                gc::ClosureValueSnapshot {
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
        self.validate_loaded_module(owner)?;
        if matches!(
            op,
            kagari_abi::operations::IterOp::New
                | kagari_abi::operations::IterOp::FromClosure
                | kagari_abi::operations::IterOp::String(_)
        ) {
            let retention = self
                .modules
                .retain_runtime_program(owner)
                .ok_or_else(|| RuntimeError::module_validation("iterator version unavailable"))?;
            if let IterOp::String(kind) = op {
                self.gc.new_string_iter(value, ty, kind, owner, retention)
            } else if op == IterOp::FromClosure {
                self.gc.new_script_iter(value, ty, owner, retention)
            } else {
                self.gc.new_iter(value, ty, owner, retention)
            }
        } else {
            self.gc.advance_iter(value, ty, op)
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
    ) -> Result<gc::ClosureValueSnapshot, RuntimeError> {
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
        value: &value::Value,
        method: &DefinitionId,
    ) -> Result<RootedInterfaceMethod, RuntimeError> {
        if let Value::Interface(id) = value
            && let Some(snapshot) = self.gc.interface_snapshot(*id)
            && !snapshot
                .methods
                .iter()
                .flatten()
                .any(|binding| &binding.method == method)
        {
            let versions = snapshot.implementation.members().collect::<Vec<_>>();
            let modules = versions
                .iter()
                .map(|version| version.bytecode.as_ref())
                .collect::<Vec<_>>();
            if let Some(parents) = bytecode::interface_ancestors(
                &snapshot.interface_type,
                &snapshot.concrete_type,
                &modules,
            ) {
                for parent in parents.into_iter().skip(1) {
                    let mut owner = method.clone();
                    owner.path.pop();
                    if owner == parent.declaration {
                        let view =
                            self.upcast_interface(value, &snapshot.interface_type, &parent)?;
                        return self.resolve_interface_method(&view, method);
                    }
                }
            }
        }
        self.resolve_interface_method_inner(value, None, |snapshot| {
            snapshot
                .methods
                .iter()
                .flatten()
                .find(|binding| &binding.method == method)
        })
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
        let versions = snapshot.implementation.members().collect::<Vec<_>>();
        let modules = versions
            .iter()
            .map(|version| version.bytecode.as_ref())
            .collect::<Vec<_>>();
        let parents = bytecode::interface_ancestors(source, &snapshot.concrete_type, &modules)
            .ok_or_else(invalid)?;
        if !parents.iter().any(|parent| parent == target) {
            return Err(invalid());
        }
        for owner in &versions {
            for (index, linked) in owner.bytecode.interface_tables.iter().enumerate() {
                let table = owner
                    .bytecode
                    .public_items
                    .iter()
                    .find_map(|item| match item {
                        PublicAbiItem::InterfaceTable(table)
                            if table.declaration == linked.declaration =>
                        {
                            table.instantiate(&linked.arguments)
                        }
                        _ => None,
                    });
                if table.is_some_and(|table| {
                    table.for_type == snapshot.concrete_type
                        && table.trait_type == AbiType::Trait(target.clone())
                }) {
                    return self.make_interface(owner, index, snapshot.data.clone());
                }
            }
        }
        Err(invalid())
    }

    /// Resolves a trait declaration's verified method ordinal without a
    /// name or declaration search during script dispatch.
    pub fn resolve_interface_method_slot(
        &self,
        value: &value::Value,
        interface: &NominalAbiType,
        slot: usize,
    ) -> Result<RootedInterfaceMethod, RuntimeError> {
        if let Value::Interface(id) = value
            && let Some(snapshot) = self.gc.interface_snapshot(*id)
            && snapshot.interface_type != *interface
        {
            let view = self.upcast_interface(value, &snapshot.interface_type, interface)?;
            return self.resolve_interface_method_slot(&view, interface, slot);
        }
        self.resolve_interface_method_inner(value, Some(interface), |snapshot| {
            snapshot.methods.get(slot).and_then(Option::as_ref)
        })
    }

    pub(super) fn resolve_interface_method_inner(
        &self,
        value: &value::Value,
        expected_interface: Option<&NominalAbiType>,
        select: impl for<'a> FnOnce(
            &'a gc::InterfaceValueSnapshot,
        ) -> Option<&'a gc::InterfaceMethodBinding>,
    ) -> Result<RootedInterfaceMethod, RuntimeError> {
        let Value::Interface(id) = value else {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "expected interface value",
            ));
        };
        let root = self.root_value(value.clone()).ok_or_else(|| {
            RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid interface handle")
        })?;
        let snapshot = self.gc.interface_snapshot(*id).ok_or_else(|| {
            RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid interface handle")
        })?;
        if expected_interface.is_some_and(|expected| *expected != snapshot.interface_type) {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "interface type does not match the call target",
            ));
        }
        let binding = select(&snapshot).cloned().ok_or_else(|| {
            RuntimeError::new(RuntimeErrorKind::ScriptTrap, "interface method unavailable")
        })?;
        Ok(RootedInterfaceMethod {
            _root: root,
            receiver: snapshot.data,
            concrete_type: snapshot.concrete_type,
            interface_type: snapshot.interface_type,
            implementation: snapshot.implementation,
            function: binding.function,
            parameter_types: binding.parameter_types,
            return_type: binding.return_type,
        })
    }

    pub fn validate_interface_method_arguments(
        &self,
        method: &RootedInterfaceMethod,
        arguments: &[value::Value],
    ) -> Result<(), RuntimeError> {
        if !method.implementation.belongs_to(self.host.owner())
            || arguments.len() != method.parameter_types.len()
            || !arguments
                .iter()
                .zip(&method.parameter_types)
                .all(|(value, ty)| {
                    self.matches_interface_method_abi(value, ty, &method.implementation)
                })
        {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "interface method argument does not match its linked signature",
            ));
        }
        Ok(())
    }

    pub fn validate_interface_method_result(
        &self,
        method: &RootedInterfaceMethod,
        result: &value::Value,
    ) -> Result<(), RuntimeError> {
        if !method.implementation.belongs_to(self.host.owner())
            || !self.matches_interface_method_abi(
                result,
                &method.return_type,
                &method.implementation,
            )
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
