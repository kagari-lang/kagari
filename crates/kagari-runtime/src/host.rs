use std::{collections::HashMap, fmt, sync::Arc};

use crate::{
    error::RuntimeError,
    metadata::{
        AbiFingerprint, FieldInfo, MethodInfo, PathAccess, TraitInfo, TypeId, TypeKind,
        TypeRegistration,
    },
    security::CapabilitySet,
    value::Value,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HostObjectId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HostFunctionId(u64);

impl HostFunctionId {
    pub fn new(index: usize) -> Self {
        Self(index as u64)
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostPassingStyle {
    Owned,
    SharedBorrow,
    UniqueBorrow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostParameter {
    pub name: &'static str,
    pub type_name: &'static str,
    pub passing: HostPassingStyle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostTypeOwnership {
    Opaque,
    Owned,
    HostRoot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostReflectionPolicy {
    Hidden,
    TypeNameOnly,
    Metadata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostTypeRegistration {
    pub script_name: String,
    pub rust_type_name: String,
    pub ownership: HostTypeOwnership,
    pub fields: Vec<FieldInfo>,
    pub methods: Vec<MethodInfo>,
    pub traits: Vec<TraitInfo>,
    pub path_access: PathAccess,
    pub reflection: HostReflectionPolicy,
    pub abi_fingerprint: AbiFingerprint,
}

impl HostTypeRegistration {
    pub fn new(script_name: impl Into<String>, rust_type_name: impl Into<String>) -> Self {
        Self {
            script_name: script_name.into(),
            rust_type_name: rust_type_name.into(),
            ownership: HostTypeOwnership::Opaque,
            fields: Vec::new(),
            methods: Vec::new(),
            traits: Vec::new(),
            path_access: PathAccess::None,
            reflection: HostReflectionPolicy::Hidden,
            abi_fingerprint: AbiFingerprint::default(),
        }
    }

    pub(crate) fn to_type_registration(&self) -> TypeRegistration {
        TypeRegistration {
            name: self.script_name.clone(),
            kind: TypeKind::HostObject,
            epoch: None,
            fields: self.fields.clone(),
            variants: Vec::new(),
            methods: self.methods.clone(),
            traits: self.traits.clone(),
            abi_fingerprint: self.abi_fingerprint,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostTypeInfo {
    pub type_id: TypeId,
    pub script_name: String,
    pub rust_type_name: String,
    pub ownership: HostTypeOwnership,
    pub fields: Vec<FieldInfo>,
    pub methods: Vec<MethodInfo>,
    pub traits: Vec<TraitInfo>,
    pub path_access: PathAccess,
    pub reflection: HostReflectionPolicy,
    pub abi_fingerprint: AbiFingerprint,
}

impl HostTypeInfo {
    pub fn from_registration(type_id: TypeId, registration: HostTypeRegistration) -> Self {
        Self {
            type_id,
            script_name: registration.script_name,
            rust_type_name: registration.rust_type_name,
            ownership: registration.ownership,
            fields: registration.fields,
            methods: registration.methods,
            traits: registration.traits,
            path_access: registration.path_access,
            reflection: registration.reflection,
            abi_fingerprint: registration.abi_fingerprint,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HostFunctionEffects {
    pub may_allocate: bool,
    pub may_trap: bool,
    pub may_call_host_services: bool,
    pub may_mutate_host_state: bool,
    pub may_suspend: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostFunctionMetadata {
    pub symbol: &'static str,
    pub params: Vec<HostParameter>,
    pub return_type: &'static str,
    pub capability_requirements: CapabilitySet,
    pub resource_cost_hint: Option<u64>,
    pub effects: HostFunctionEffects,
    pub abi_fingerprint: AbiFingerprint,
}

impl HostFunctionMetadata {
    pub fn new(
        symbol: &'static str,
        params: Vec<HostParameter>,
        return_type: &'static str,
    ) -> Self {
        Self {
            symbol,
            params,
            return_type,
            capability_requirements: CapabilitySet::default(),
            resource_cost_hint: None,
            effects: HostFunctionEffects::default(),
            abi_fingerprint: AbiFingerprint::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostError {
    message: String,
}

impl HostError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

pub type HostCallback = dyn Fn(&[Value]) -> Result<Value, HostError> + Send + Sync + 'static;

#[derive(Clone)]
pub struct HostFunction {
    id: Option<HostFunctionId>,
    metadata: HostFunctionMetadata,
    handler: Arc<HostCallback>,
}

impl fmt::Debug for HostFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostFunction")
            .field("symbol", &self.metadata.symbol)
            .field("id", &self.id)
            .field("params", &self.metadata.params)
            .field("return_type", &self.metadata.return_type)
            .field(
                "capability_requirements",
                &self.metadata.capability_requirements,
            )
            .field("resource_cost_hint", &self.metadata.resource_cost_hint)
            .field("effects", &self.metadata.effects)
            .field("abi_fingerprint", &self.metadata.abi_fingerprint)
            .finish_non_exhaustive()
    }
}

impl HostFunction {
    pub fn new(
        symbol: &'static str,
        params: Vec<HostParameter>,
        return_type: &'static str,
        handler: impl Fn(&[Value]) -> Result<Value, HostError> + Send + Sync + 'static,
    ) -> Self {
        Self::with_metadata(
            HostFunctionMetadata::new(symbol, params, return_type),
            handler,
        )
    }

    pub fn with_metadata(
        metadata: HostFunctionMetadata,
        handler: impl Fn(&[Value]) -> Result<Value, HostError> + Send + Sync + 'static,
    ) -> Self {
        Self {
            id: None,
            metadata,
            handler: Arc::new(handler),
        }
    }

    pub fn id(&self) -> Option<HostFunctionId> {
        self.id
    }

    pub fn metadata(&self) -> &HostFunctionMetadata {
        &self.metadata
    }

    pub fn symbol(&self) -> &str {
        self.metadata.symbol
    }

    pub fn invoke(&self, args: &[Value]) -> Result<Value, HostError> {
        (self.handler)(args)
    }

    fn assign_id(&mut self, id: HostFunctionId) {
        self.id = Some(id);
    }
}

#[derive(Debug, Default)]
pub struct HostRegistry {
    next_function_id: usize,
    functions: HashMap<String, HostFunction>,
    types: HashMap<TypeId, HostTypeInfo>,
    type_names: HashMap<String, TypeId>,
}

impl HostRegistry {
    pub fn register(&mut self, mut function: HostFunction) -> Result<HostFunctionId, RuntimeError> {
        let symbol = function.metadata.symbol.to_owned();
        if self.functions.contains_key(&symbol) {
            return Err(RuntimeError::metadata_conflict(symbol));
        }
        let id = HostFunctionId::new(self.next_function_id);
        self.next_function_id += 1;
        function.assign_id(id);
        self.functions.insert(symbol, function);
        Ok(id)
    }

    pub fn register_type(&mut self, info: HostTypeInfo) -> Result<(), RuntimeError> {
        if self.types.contains_key(&info.type_id) || self.type_names.contains_key(&info.script_name)
        {
            return Err(RuntimeError::metadata_conflict(info.script_name));
        }
        self.type_names
            .insert(info.script_name.clone(), info.type_id);
        self.types.insert(info.type_id, info);
        Ok(())
    }

    pub fn function(&self, symbol: &str) -> Option<&HostFunction> {
        self.functions.get(symbol)
    }

    pub fn functions(&self) -> impl Iterator<Item = &HostFunction> {
        self.functions.values()
    }

    pub fn host_type(&self, type_id: TypeId) -> Option<&HostTypeInfo> {
        self.types.get(&type_id)
    }

    pub fn host_type_by_name(&self, script_name: &str) -> Option<&HostTypeInfo> {
        let type_id = self.type_names.get(script_name)?;
        self.types.get(type_id)
    }

    pub fn host_types(&self) -> impl Iterator<Item = &HostTypeInfo> {
        self.types.values()
    }

    pub fn invoke(&self, symbol: &str, args: &[Value]) -> Result<Value, HostError> {
        let function = self
            .functions
            .get(symbol)
            .ok_or_else(|| HostError::new(format!("unknown host function `{symbol}`")))?;
        function.invoke(args)
    }
}

#[derive(Debug)]
pub struct SharedHostRef<'host, T: ?Sized> {
    value: &'host T,
}

impl<'host, T: ?Sized> SharedHostRef<'host, T> {
    pub fn new(value: &'host T) -> Self {
        Self { value }
    }

    pub fn get(&self) -> &'host T {
        self.value
    }
}

#[derive(Debug)]
pub struct MutHostRef<'host, T: ?Sized> {
    value: &'host mut T,
}

impl<'host, T: ?Sized> MutHostRef<'host, T> {
    pub fn new(value: &'host mut T) -> Self {
        Self { value }
    }

    pub fn get(&self) -> &T {
        self.value
    }

    pub fn get_mut(&mut self) -> &mut T {
        self.value
    }
}
