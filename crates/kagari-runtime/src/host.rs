use std::{cell::RefCell, collections::HashMap, fmt, sync::Arc};

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
pub struct HostFrameId(u64);

impl HostFrameId {
    pub fn new(index: usize) -> Self {
        Self(index as u64)
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BorrowEpoch(u64);

impl BorrowEpoch {
    pub fn new(index: usize) -> Self {
        Self(index as u64)
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostBorrowKind {
    Shared,
    Unique,
}

impl HostBorrowKind {
    pub fn satisfies(self, required: Self) -> bool {
        matches!(
            (self, required),
            (Self::Shared, Self::Shared)
                | (Self::Unique, Self::Shared)
                | (Self::Unique, Self::Unique)
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FrameHostBorrowToken {
    frame_id: HostFrameId,
    object_id: HostObjectId,
    borrow_kind: HostBorrowKind,
    type_id: TypeId,
    epoch: BorrowEpoch,
}

impl FrameHostBorrowToken {
    fn new(
        frame_id: HostFrameId,
        object_id: HostObjectId,
        borrow_kind: HostBorrowKind,
        type_id: TypeId,
        epoch: BorrowEpoch,
    ) -> Self {
        Self {
            frame_id,
            object_id,
            borrow_kind,
            type_id,
            epoch,
        }
    }

    pub fn frame_id(self) -> HostFrameId {
        self.frame_id
    }

    pub fn object_id(self) -> HostObjectId {
        self.object_id
    }

    pub fn borrow_kind(self) -> HostBorrowKind {
        self.borrow_kind
    }

    pub fn type_id(self) -> TypeId {
        self.type_id
    }

    pub fn epoch(self) -> BorrowEpoch {
        self.epoch
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FrameBorrowRecord {
    object_id: HostObjectId,
    borrow_kind: HostBorrowKind,
    type_id: TypeId,
}

impl FrameBorrowRecord {
    fn from_token(token: FrameHostBorrowToken) -> Self {
        Self {
            object_id: token.object_id,
            borrow_kind: token.borrow_kind,
            type_id: token.type_id,
        }
    }

    fn matches(self, token: FrameHostBorrowToken) -> bool {
        self == Self::from_token(token)
    }
}

#[derive(Debug)]
struct ActiveBorrowFrame {
    epoch: BorrowEpoch,
    borrows: Vec<FrameBorrowRecord>,
}

#[derive(Debug, Default)]
struct ObjectBorrowState {
    shared_count: usize,
    unique_count: usize,
}

impl ObjectBorrowState {
    fn is_empty(&self) -> bool {
        self.shared_count == 0 && self.unique_count == 0
    }
}

#[derive(Debug, Default)]
struct HostBorrowState {
    next_frame_id: u64,
    next_epoch: u64,
    active_frames: HashMap<HostFrameId, ActiveBorrowFrame>,
    object_borrows: HashMap<HostObjectId, ObjectBorrowState>,
}

#[derive(Debug, Default)]
pub struct HostBorrowTable {
    state: RefCell<HostBorrowState>,
}

impl HostBorrowTable {
    pub fn enter_frame(&self) -> HostCallGuard<'_> {
        let mut state = self.state.borrow_mut();
        let frame_id = HostFrameId(state.next_frame_id);
        let epoch = BorrowEpoch(state.next_epoch);
        state.next_frame_id += 1;
        state.next_epoch += 1;
        state.active_frames.insert(
            frame_id,
            ActiveBorrowFrame {
                epoch,
                borrows: Vec::new(),
            },
        );
        HostCallGuard {
            table: self,
            frame_id,
            epoch,
        }
    }

    pub fn validate(
        &self,
        token: FrameHostBorrowToken,
        required_kind: HostBorrowKind,
    ) -> Result<(), RuntimeError> {
        let state = self.state.borrow();
        let frame = state.active_frames.get(&token.frame_id).ok_or_else(|| {
            RuntimeError::expired_host_borrow(format!(
                "frame {} is no longer active",
                token.frame_id.index()
            ))
        })?;
        if frame.epoch != token.epoch {
            return Err(RuntimeError::expired_host_borrow(format!(
                "frame {} epoch mismatch",
                token.frame_id.index()
            )));
        }
        if !token.borrow_kind.satisfies(required_kind) {
            return Err(RuntimeError::host_borrow_conflict(format!(
                "{:?} borrow cannot satisfy {:?} access",
                token.borrow_kind, required_kind
            )));
        }
        if !frame
            .borrows
            .iter()
            .copied()
            .any(|record| record.matches(token))
        {
            return Err(RuntimeError::expired_host_borrow(format!(
                "token for host object {} is not active",
                token.object_id.0
            )));
        }
        Ok(())
    }

    pub fn validate_no_escape(value: &Value) -> Result<(), RuntimeError> {
        if value.contains_host_borrow() {
            Err(RuntimeError::host_borrow_escape(
                "frame-scoped host borrow cannot outlive its call frame",
            ))
        } else {
            Ok(())
        }
    }

    fn borrow(
        &self,
        frame_id: HostFrameId,
        epoch: BorrowEpoch,
        object_id: HostObjectId,
        borrow_kind: HostBorrowKind,
        type_id: TypeId,
    ) -> Result<FrameHostBorrowToken, RuntimeError> {
        let mut state = self.state.borrow_mut();
        let frame = state.active_frames.get(&frame_id).ok_or_else(|| {
            RuntimeError::expired_host_borrow(format!(
                "frame {} is no longer active",
                frame_id.index()
            ))
        })?;
        if frame.epoch != epoch {
            return Err(RuntimeError::expired_host_borrow(format!(
                "frame {} epoch mismatch",
                frame_id.index()
            )));
        }

        let object_state = state.object_borrows.entry(object_id).or_default();
        match borrow_kind {
            HostBorrowKind::Shared if object_state.unique_count > 0 => {
                return Err(RuntimeError::host_borrow_conflict(format!(
                    "host object {} already has an active unique borrow",
                    object_id.0
                )));
            }
            HostBorrowKind::Shared => {
                object_state.shared_count += 1;
            }
            HostBorrowKind::Unique
                if object_state.shared_count > 0 || object_state.unique_count > 0 =>
            {
                return Err(RuntimeError::host_borrow_conflict(format!(
                    "host object {} already has an active borrow",
                    object_id.0
                )));
            }
            HostBorrowKind::Unique => {
                object_state.unique_count += 1;
            }
        }

        let token = FrameHostBorrowToken::new(frame_id, object_id, borrow_kind, type_id, epoch);
        state
            .active_frames
            .get_mut(&frame_id)
            .expect("checked active host borrow frame before recording token")
            .borrows
            .push(FrameBorrowRecord::from_token(token));
        Ok(token)
    }

    fn leave_frame(&self, frame_id: HostFrameId, epoch: BorrowEpoch) {
        let mut state = self.state.borrow_mut();
        let Some(frame) = state.active_frames.remove(&frame_id) else {
            return;
        };
        if frame.epoch != epoch {
            debug_assert_eq!(frame.epoch, epoch);
            return;
        }

        for record in frame.borrows {
            let mut remove_object = false;
            if let Some(object_state) = state.object_borrows.get_mut(&record.object_id) {
                match record.borrow_kind {
                    HostBorrowKind::Shared => {
                        object_state.shared_count = object_state.shared_count.saturating_sub(1);
                    }
                    HostBorrowKind::Unique => {
                        object_state.unique_count = object_state.unique_count.saturating_sub(1);
                    }
                }
                remove_object = object_state.is_empty();
            }
            if remove_object {
                state.object_borrows.remove(&record.object_id);
            }
        }
    }
}

#[derive(Debug)]
pub struct HostCallGuard<'host> {
    table: &'host HostBorrowTable,
    frame_id: HostFrameId,
    epoch: BorrowEpoch,
}

impl<'host> HostCallGuard<'host> {
    pub fn frame_id(&self) -> HostFrameId {
        self.frame_id
    }

    pub fn epoch(&self) -> BorrowEpoch {
        self.epoch
    }

    pub fn borrow_shared(
        &self,
        object_id: HostObjectId,
        type_id: TypeId,
    ) -> Result<FrameHostBorrowToken, RuntimeError> {
        self.table.borrow(
            self.frame_id,
            self.epoch,
            object_id,
            HostBorrowKind::Shared,
            type_id,
        )
    }

    pub fn borrow_unique(
        &self,
        object_id: HostObjectId,
        type_id: TypeId,
    ) -> Result<FrameHostBorrowToken, RuntimeError> {
        self.table.borrow(
            self.frame_id,
            self.epoch,
            object_id,
            HostBorrowKind::Unique,
            type_id,
        )
    }

    pub fn validate(
        &self,
        token: FrameHostBorrowToken,
        required_kind: HostBorrowKind,
    ) -> Result<(), RuntimeError> {
        if token.frame_id != self.frame_id {
            return Err(RuntimeError::expired_host_borrow(format!(
                "token frame {} does not match current frame {}",
                token.frame_id.index(),
                self.frame_id.index()
            )));
        }
        self.table.validate(token, required_kind)
    }

    pub fn validate_no_escape(value: &Value) -> Result<(), RuntimeError> {
        HostBorrowTable::validate_no_escape(value)
    }
}

impl Drop for HostCallGuard<'_> {
    fn drop(&mut self) {
        self.table.leave_frame(self.frame_id, self.epoch);
    }
}

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
