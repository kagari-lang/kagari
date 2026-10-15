mod borrows;
mod installation;
mod registry;
pub mod scoped;
mod views;
use crate::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    error_trace::ErrorTrace,
    gc::GcHeap,
    host::borrows::BorrowObjectKey,
    host_scope::HostResourceScope,
    metadata::{AbiFingerprint, FieldMetadataId, TypeId},
    numeric,
    resource::ResourceState,
    value::Value,
};
use kagari_bytecode::instruction::BinaryOp;
use kagari_common::identity::{
    map::DefinitionContext,
    table::{DefinitionId, DefinitionTable},
};

use kagari_types::declaration::{
    host::{HostTypePolicy, PathAccess},
    path::{HostPathDeclaration, HostPathSegmentDeclaration},
};
use std::{
    cell::RefCell,
    collections::HashMap,
    fmt, iter,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

mod path_fingerprint;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HostObjectId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HostSchemaEpoch(u64);

impl HostSchemaEpoch {
    pub fn new(index: usize) -> Self {
        Self(index as u64)
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HostRootHandle {
    owner: HostRegistryId,
    object_id: HostObjectId,
    type_id: TypeId,
    schema_epoch: HostSchemaEpoch,
    abi_fingerprint: AbiFingerprint,
}

impl HostRootHandle {
    pub(crate) fn new(
        owner: HostRegistryId,
        object_id: HostObjectId,
        type_id: TypeId,
        schema_epoch: HostSchemaEpoch,
        abi_fingerprint: AbiFingerprint,
    ) -> Self {
        Self {
            owner,
            object_id,
            type_id,
            schema_epoch,
            abi_fingerprint,
        }
    }

    pub fn object_id(self) -> HostObjectId {
        self.object_id
    }

    pub fn type_id(self) -> TypeId {
        self.type_id
    }

    pub fn schema_epoch(self) -> HostSchemaEpoch {
        self.schema_epoch
    }

    pub fn abi_fingerprint(self) -> AbiFingerprint {
        self.abi_fingerprint
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HostPathDescriptorId(u64);

impl HostPathDescriptorId {
    pub fn new(index: usize) -> Self {
        Self(index as u64)
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DynamicPathArgSlot(u32);

impl DynamicPathArgSlot {
    pub fn new(index: usize) -> Self {
        Self(index as u32)
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DynamicPathArgument {
    pub ty: TypeId,
    pub value: Value,
}

impl DynamicPathArgument {
    pub fn new(ty: TypeId, value: Value) -> Self {
        Self { ty, value }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DynamicPathArguments {
    args: Vec<DynamicPathArgument>,
}

impl DynamicPathArguments {
    pub fn new(args: Vec<DynamicPathArgument>) -> Self {
        Self { args }
    }

    pub fn empty() -> Self {
        Self { args: Vec::new() }
    }

    pub fn as_slice(&self) -> &[DynamicPathArgument] {
        &self.args
    }

    pub fn len(&self) -> usize {
        self.args.len()
    }

    pub fn is_empty(&self) -> bool {
        self.args.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostPathOperation {
    Read,
    Set,
    Modify(BinaryOp),
    MakeView,
}

impl HostPathOperation {
    pub fn writes(self) -> bool {
        matches!(self, Self::Set | Self::Modify(_))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DynamicPathParameter {
    pub slot: DynamicPathArgSlot,
    pub ty: TypeId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostPathSegment {
    Field {
        name: String,
        field_id: FieldMetadataId,
        owner_type: TypeId,
        result_type: TypeId,
        access: PathAccess,
        abi_fingerprint: AbiFingerprint,
    },
    Index {
        slot: DynamicPathArgSlot,
        collection_type: TypeId,
        index_type: TypeId,
        result_type: TypeId,
        access: PathAccess,
    },
    Virtual {
        name: String,
        result_type: TypeId,
        access: PathAccess,
    },
}

impl HostPathSegment {
    pub fn result_type(&self) -> TypeId {
        match self {
            Self::Field { result_type, .. }
            | Self::Index { result_type, .. }
            | Self::Virtual { result_type, .. } => *result_type,
        }
    }

    pub fn access(&self) -> PathAccess {
        match self {
            Self::Field { access, .. }
            | Self::Index { access, .. }
            | Self::Virtual { access, .. } => *access,
        }
    }

    pub fn abi_fingerprint(&self) -> AbiFingerprint {
        match self {
            Self::Field {
                abi_fingerprint, ..
            } => *abi_fingerprint,
            Self::Index { .. } | Self::Virtual { .. } => AbiFingerprint(0),
        }
    }

    fn dynamic_parameter(&self) -> Option<DynamicPathParameter> {
        match self {
            Self::Index {
                slot, index_type, ..
            } => Some(DynamicPathParameter {
                slot: *slot,
                ty: *index_type,
            }),
            Self::Field { .. } | Self::Virtual { .. } => None,
        }
    }
}

/// Scoped path segments plus physical type slots from the receiving runtime.
/// Registration imports identities, validates the complete contract, then publishes.
#[derive(Debug, Clone)]
pub struct HostPathDescriptorRegistration {
    pub definitions: DefinitionTable,
    pub root_type: TypeId,
    pub result_type: TypeId,
    pub segments: Vec<HostPathSegmentDeclaration>,
    pub access: PathAccess,
    pub schema_epoch: HostSchemaEpoch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostPathDescriptor {
    pub declaration: HostPathDeclaration,
    pub id: HostPathDescriptorId,
    pub root_type: TypeId,
    pub result_type: TypeId,
    pub segments: Vec<HostPathSegment>,
    pub dynamic_parameters: Vec<DynamicPathParameter>,
    pub access: PathAccess,
    pub schema_epoch: HostSchemaEpoch,
    pub abi_fingerprint: AbiFingerprint,
}

impl HostPathDescriptor {
    fn from_registration(
        id: HostPathDescriptorId,
        registration: HostPathDescriptorRegistration,
        segments: Vec<HostPathSegment>,
        abi_fingerprint: AbiFingerprint,
        declaration: HostPathDeclaration,
    ) -> Result<Self, RuntimeError> {
        validate_path_access(registration.access, "path descriptor")?;
        let dynamic_parameters = collect_dynamic_parameters(&segments)?;
        let Some(last_segment) = segments.last() else {
            return Err(RuntimeError::typed_path_validation(
                "path descriptor must contain at least one segment",
            ));
        };
        if last_segment.result_type() != registration.result_type {
            return Err(RuntimeError::typed_path_validation(
                "path descriptor result type does not match its final segment",
            ));
        }
        let mut current_type = registration.root_type;
        for segment in &segments {
            let input_type = match segment {
                HostPathSegment::Field { owner_type, .. } => *owner_type,
                HostPathSegment::Index {
                    collection_type, ..
                } => *collection_type,
                HostPathSegment::Virtual { .. } => current_type,
            };
            if input_type != current_type {
                return Err(RuntimeError::typed_path_validation(
                    "path segment input type does not match the preceding result",
                ));
            }
            validate_path_access(segment.access(), "path segment")?;
            if !path_access_allows(segment.access(), registration.access) {
                return Err(RuntimeError::typed_path_validation(
                    "path descriptor access exceeds a segment access policy",
                ));
            }
            current_type = segment.result_type();
        }
        Ok(Self {
            id,
            declaration,
            root_type: registration.root_type,
            result_type: registration.result_type,
            segments,
            dynamic_parameters,
            access: registration.access,
            schema_epoch: registration.schema_epoch,
            abi_fingerprint,
        })
    }

    pub fn requires_dynamic_args(&self) -> bool {
        !self.dynamic_parameters.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct HostPathViewHandle {
    root: HostRootHandle,
    base: Option<Arc<HostPathViewHandle>>,
    descriptor_id: HostPathDescriptorId,
    result_type: TypeId,
    access: PathAccess,
    schema_epoch: HostSchemaEpoch,
    dynamic_args: DynamicPathArguments,
}

impl HostPathViewHandle {
    fn new(
        root: HostRootHandle,
        base: Option<Arc<HostPathViewHandle>>,
        descriptor: &HostPathDescriptor,
        dynamic_args: DynamicPathArguments,
    ) -> Self {
        let access = match base.as_ref().map(|view| view.access()) {
            Some(PathAccess::ReadOnly) => PathAccess::ReadOnly,
            _ => descriptor.access,
        };
        Self {
            root,
            base,
            descriptor_id: descriptor.id,
            result_type: descriptor.result_type,
            access,
            schema_epoch: descriptor.schema_epoch,
            dynamic_args,
        }
    }

    pub fn root(&self) -> HostRootHandle {
        self.root
    }

    pub fn base(&self) -> Option<&HostPathViewHandle> {
        self.base.as_deref()
    }

    pub fn descriptor_id(&self) -> HostPathDescriptorId {
        self.descriptor_id
    }

    pub fn result_type(&self) -> TypeId {
        self.result_type
    }

    pub fn access(&self) -> PathAccess {
        self.access
    }

    pub fn schema_epoch(&self) -> HostSchemaEpoch {
        self.schema_epoch
    }

    pub fn dynamic_args(&self) -> &DynamicPathArguments {
        &self.dynamic_args
    }

    pub(crate) fn retained_values(&self) -> impl Iterator<Item = &Value> {
        iter::successors(Some(self), |view| view.base())
            .flat_map(|view| view.dynamic_args().as_slice())
            .map(|arg| &arg.value)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct HostPathContext {
    pub root: HostRootHandle,
    pub base_view: Option<Arc<HostPathViewHandle>>,
    pub descriptor: HostPathDescriptor,
    pub dynamic_args: DynamicPathArguments,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HostPathMutationRecord {
    pub root: HostRootHandle,
    pub base_view: Option<Arc<HostPathViewHandle>>,
    pub descriptor_id: HostPathDescriptorId,
    pub operation: HostPathOperation,
    pub dynamic_args: DynamicPathArguments,
    pub old_value: Option<Value>,
    pub new_value: Value,
}

pub type HostPathReadCallback = dyn Fn(&HostCallContext<'_>, &HostPathContext) -> Result<Value, HostError>
    + Send
    + Sync
    + 'static;

/// Preparation validates and reserves host resources without changing the target.
pub type HostPathPrepareWriteCallback = dyn Fn(
        &HostCallContext<'_>,
        &HostPathContext,
        &HostPathMutationRecord,
    ) -> Result<PreparedHostPathWrite, HostError>
    + Send
    + Sync
    + 'static;

/// A prepared target update. The action must only commit prepared host state;
/// it must not allocate fallibly, invoke scripts, or return a business failure.
/// Panics and attempts to execute through the runtime quarantine that runtime.
#[must_use = "prepared writes must be returned to the runtime or dropped to release reservations"]
pub struct PreparedHostPathWrite(Box<dyn FnOnce()>);

impl PreparedHostPathWrite {
    pub fn new(commit: impl FnOnce() + 'static) -> Self {
        Self(Box::new(commit))
    }

    fn commit(self) {
        (self.0)()
    }
}

pub type HostPathValidateCallback = dyn Fn(
        &HostCallContext<'_>,
        &HostPathContext,
        HostPathOperation,
        Option<&Value>,
    ) -> Result<(), HostError>
    + Send
    + Sync
    + 'static;

#[derive(Clone, Default)]
pub struct HostPathAdapter {
    read: Option<Arc<HostPathReadCallback>>,
    prepare_write: Option<Arc<HostPathPrepareWriteCallback>>,
    validate: Option<Arc<HostPathValidateCallback>>,
}

impl fmt::Debug for HostPathAdapter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostPathAdapter")
            .field("has_read", &self.read.is_some())
            .field("has_prepare_write", &self.prepare_write.is_some())
            .field("has_validate", &self.validate.is_some())
            .finish()
    }
}

impl HostPathAdapter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_read(
        mut self,
        read: impl Fn(&HostCallContext<'_>, &HostPathContext) -> Result<Value, HostError>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        self.read = Some(Arc::new(read));
        self
    }

    pub fn with_prepare_write(
        mut self,
        prepare: impl Fn(
            &HostCallContext<'_>,
            &HostPathContext,
            &HostPathMutationRecord,
        ) -> Result<PreparedHostPathWrite, HostError>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        self.prepare_write = Some(Arc::new(prepare));
        self
    }

    pub fn with_validate(
        mut self,
        validate: impl Fn(
            &HostCallContext<'_>,
            &HostPathContext,
            HostPathOperation,
            Option<&Value>,
        ) -> Result<(), HostError>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        self.validate = Some(Arc::new(validate));
        self
    }
}

fn collect_dynamic_parameters(
    segments: &[HostPathSegment],
) -> Result<Vec<DynamicPathParameter>, RuntimeError> {
    let mut parameters = Vec::<DynamicPathParameter>::new();
    for segment in segments {
        let Some(parameter) = segment.dynamic_parameter() else {
            continue;
        };
        if let Some(existing) = parameters
            .iter()
            .find(|existing| existing.slot == parameter.slot)
        {
            if existing.ty != parameter.ty {
                return Err(RuntimeError::typed_path_validation(
                    "dynamic path argument slot used with multiple types",
                ));
            }
            continue;
        }
        parameters.push(parameter);
    }
    parameters.sort_by_key(|parameter| parameter.slot.index());
    for (expected, parameter) in parameters.iter().enumerate() {
        if parameter.slot.index() != expected {
            return Err(RuntimeError::typed_path_validation(
                "dynamic path argument slots must be contiguous from zero",
            ));
        }
    }
    Ok(parameters)
}

fn validate_dynamic_arguments(
    heap: &GcHeap,
    descriptor: &HostPathDescriptor,
    args: &DynamicPathArguments,
) -> Result<(), RuntimeError> {
    if descriptor.dynamic_parameters.len() != args.len() {
        return Err(RuntimeError::typed_path_validation(format!(
            "path descriptor expects {} dynamic arguments, found {}",
            descriptor.dynamic_parameters.len(),
            args.len()
        )));
    }
    for (parameter, arg) in descriptor.dynamic_parameters.iter().zip(args.as_slice()) {
        if parameter.ty != arg.ty {
            return Err(RuntimeError::typed_path_validation(format!(
                "dynamic argument {} has the wrong type",
                parameter.slot.index()
            )));
        }
        if !arg.value.is_storable(heap) {
            return Err(RuntimeError::typed_path_validation(
                "dynamic path arguments must be storable script values",
            ));
        }
    }
    Ok(())
}

fn dynamic_args_for_descriptor(
    descriptor: &HostPathDescriptor,
    values: Vec<Value>,
) -> Result<DynamicPathArguments, RuntimeError> {
    if descriptor.dynamic_parameters.len() != values.len() {
        return Err(RuntimeError::typed_path_validation(format!(
            "path descriptor expects {} dynamic arguments, found {}",
            descriptor.dynamic_parameters.len(),
            values.len()
        )));
    }
    Ok(DynamicPathArguments::new(
        descriptor
            .dynamic_parameters
            .iter()
            .zip(values)
            .map(|(parameter, value)| DynamicPathArgument::new(parameter.ty, value))
            .collect(),
    ))
}

fn validate_path_access(access: PathAccess, context: &str) -> Result<(), RuntimeError> {
    if access == PathAccess::None {
        Err(RuntimeError::typed_path_validation(format!(
            "{context} has no path access"
        )))
    } else {
        Ok(())
    }
}

fn path_access_allows(available: PathAccess, required: PathAccess) -> bool {
    matches!(
        (available, required),
        (PathAccess::ReadOnly, PathAccess::ReadOnly)
            | (PathAccess::ReadWrite, PathAccess::ReadOnly)
            | (PathAccess::ReadWrite, PathAccess::ReadWrite)
    )
}

fn apply_path_modify(op: BinaryOp, old_value: Value, rhs: Value) -> Result<Value, RuntimeError> {
    numeric::binary(op, old_value, rhs)
}

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
    owner: HostBorrowOwner,
    frame_id: HostFrameId,
    object_id: HostObjectId,
    borrow_kind: HostBorrowKind,
    type_id: TypeId,
    epoch: BorrowEpoch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FrameBorrowRecord {
    object_id: HostObjectId,
    borrow_kind: HostBorrowKind,
    type_id: Option<TypeId>,
    path: Arc<[DefinitionId]>,
}

#[derive(Debug)]
struct ActiveBorrowFrame {
    epoch: BorrowEpoch,
    borrows: Vec<FrameBorrowRecord>,
    roots: Vec<scoped::ScopedRoot>,
}

#[derive(Debug, Default)]
struct ObjectBorrowState {
    paths: Vec<PathBorrowState>,
}

#[derive(Debug)]
struct PathBorrowState {
    path: Arc<[DefinitionId]>,
    shared_count: usize,
    unique_count: usize,
}

impl ObjectBorrowState {
    fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }
}

#[derive(Debug, Default)]
struct HostBorrowState {
    next_frame_id: u64,
    next_epoch: u64,
    active_frames: HashMap<HostFrameId, ActiveBorrowFrame>,
    object_borrows: HashMap<BorrowObjectKey, ObjectBorrowState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct HostBorrowOwner(u64);

impl Default for HostBorrowOwner {
    fn default() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(
            NEXT.try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                .expect("host borrow ownership exhausted"),
        )
    }
}

#[derive(Debug, Default)]
pub struct HostBorrowTable {
    owner: HostBorrowOwner,
    state: RefCell<HostBorrowState>,
}

/// A scoped borrow into its table; it cannot keep the table alive after teardown.
///
/// ```compile_fail
/// use kagari_runtime::host::HostBorrowTable;
/// let table = HostBorrowTable::default();
/// let guard = table.enter_frame().unwrap();
/// drop(table);
/// drop(guard);
/// ```
#[derive(Debug)]
pub struct HostCallGuard<'runtime> {
    table: &'runtime HostBorrowTable,
    resources: Option<&'runtime ResourceState>,
    frame_id: HostFrameId,
    epoch: BorrowEpoch,
}

impl HostCallGuard<'_> {
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
        self.table
            .borrow(self, object_id, HostBorrowKind::Shared, type_id)
    }

    pub fn borrow_unique(
        &self,
        object_id: HostObjectId,
        type_id: TypeId,
    ) -> Result<FrameHostBorrowToken, RuntimeError> {
        self.table
            .borrow(self, object_id, HostBorrowKind::Unique, type_id)
    }

    pub fn validate(
        &self,
        token: FrameHostBorrowToken,
        required_kind: HostBorrowKind,
    ) -> Result<(), RuntimeError> {
        if let Some(resources) = self.resources {
            resources.ensure_execution_allowed()?;
        }
        if token.frame_id != self.frame_id {
            return Err(RuntimeError::expired_host_borrow(format!(
                "token frame {} does not match current frame {}",
                token.frame_id.index(),
                self.frame_id.index()
            )));
        }
        self.table.validate(token, required_kind)
    }

    pub fn validate_no_escape(heap: &GcHeap, value: &Value) -> Result<(), RuntimeError> {
        HostBorrowTable::validate_no_escape(heap, value)
    }
}

impl Drop for HostCallGuard<'_> {
    fn drop(&mut self) {
        self.table
            .leave_frame(self.frame_id, self.epoch, self.resources);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct HostRegistryId(u64);

impl Default for HostRegistryId {
    fn default() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(
            NEXT.try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                .expect("host registry identity exhausted"),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostError {
    message: String,
    trace: Option<Arc<ErrorTrace>>,
}

impl HostError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            trace: None,
        }
    }

    pub fn trace(&self) -> Option<&Arc<ErrorTrace>> {
        self.trace.as_ref()
    }

    pub fn with_trace(mut self, trace: Arc<ErrorTrace>) -> Self {
        if self
            .trace
            .as_ref()
            .is_none_or(|previous| previous.frames.is_empty())
        {
            self.trace = Some(trace);
        }
        self
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

/// Available only while a checked runtime host call is active.
pub struct HostCallContext<'a> {
    scope: HostResourceScope<'a>,
}

impl<'a> HostCallContext<'a> {
    pub(crate) fn new(runtime: &'a Runtime, args: &[Value]) -> Result<Self, RuntimeError> {
        Ok(Self {
            scope: runtime.host_scope(args)?,
        })
    }

    pub fn runtime(&self) -> &'a Runtime {
        self.scope.runtime()
    }

    pub fn borrows(&self) -> &HostCallGuard<'_> {
        self.scope.borrows()
    }

    pub fn retain_temporaries(&self, values: &[Value]) -> Result<(), RuntimeError> {
        self.scope.retain_values(values)
    }

    pub fn execution_time_millis(&self) -> Result<i64, RuntimeError> {
        self.runtime().execution_time_millis()
    }

    pub fn next_execution_random_u64(&self) -> Result<u64, RuntimeError> {
        self.runtime().next_execution_random_u64()
    }
}

#[derive(Debug)]
pub struct HostRegistry {
    definitions: DefinitionContext,
    owner: HostRegistryId,
    next_path_descriptor_id: usize,
    type_declarations: HashMap<DefinitionId, TypeId>,
    registered_types: HashMap<TypeId, RegisteredHostType>,
    roots: HashMap<HostObjectId, HostRootHandle>,
    path_descriptors: HashMap<HostPathDescriptorId, HostPathDescriptor>,
    path_adapters: HashMap<HostPathDescriptorId, HostPathAdapter>,
    dirty_paths: RefCell<Vec<HostPathMutationRecord>>,
}

/// Runtime slots derived from checked declaration owners, without another schema
/// or Rust object store. The legacy producer is retired with its HR03 consumers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RegisteredHostType {
    policy: HostTypePolicy,
    abi_fingerprint: AbiFingerprint,
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

fn path_scope_error(error: RuntimeError) -> RuntimeError {
    if error.kind() == RuntimeErrorKind::HostCallFailure {
        RuntimeError::typed_path_validation(error.message())
    } else {
        error
    }
}
