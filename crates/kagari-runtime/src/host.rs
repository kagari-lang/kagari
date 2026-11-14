mod borrows;
pub(crate) mod installation;
mod path_calls;
mod paths;
mod registry;
pub mod scoped;
mod views;
use crate::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    error_trace::ErrorTrace,
    host::borrows::BorrowObjectKey,
    host_scope::HostResourceScope,
    metadata::{AbiFingerprint, FieldMetadataId, TypeId},
    module::ExecutableModule,
    native::conversion::{arguments::ArgumentValues, context::ConversionContext},
    numeric,
    resource::ResourceState,
    value::{carriers::RootedValue, view::ValueView},
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
    fmt,
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

#[derive(Debug, Clone)]
pub struct DynamicPathArgument<V> {
    pub ty: TypeId,
    pub value: V,
}

impl<V> DynamicPathArgument<V> {
    pub fn new(ty: TypeId, value: V) -> Self {
        Self { ty, value }
    }
}

#[derive(Debug, Clone)]
pub struct DynamicPathArguments<V> {
    args: Vec<DynamicPathArgument<V>>,
}

impl<V> DynamicPathArguments<V> {
    pub fn new(args: Vec<DynamicPathArgument<V>>) -> Self {
        Self { args }
    }

    pub fn empty() -> Self {
        Self { args: Vec::new() }
    }

    pub fn as_slice(&self) -> &[DynamicPathArgument<V>] {
        &self.args
    }

    pub fn len(&self) -> usize {
        self.args.len()
    }

    pub fn is_empty(&self) -> bool {
        self.args.is_empty()
    }
}

impl<V> Default for DynamicPathArguments<V> {
    fn default() -> Self {
        Self::empty()
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

/// A callback snapshot borrows protected values; a durable mutation record uses
/// the same labeled fields with genuine RootedValue ownership. Base values are
/// actual path objects, never recursively copied detached descriptor chains.
#[derive(Debug, Clone)]
pub struct HostPathContext<V> {
    pub root: HostRootHandle,
    pub base_view: Option<V>,
    pub descriptor: HostPathDescriptor,
    pub access: PathAccess,
    pub dynamic_args: DynamicPathArguments<V>,
}

#[derive(Debug, Clone)]
pub struct HostPathMutationRecord<V> {
    pub root: HostRootHandle,
    pub base_view: Option<V>,
    pub descriptor_id: HostPathDescriptorId,
    pub operation: HostPathOperation,
    pub dynamic_args: DynamicPathArguments<V>,
    pub old_value: Option<V>,
    pub new_value: V,
}

pub type HostPathReadCallback = dyn for<'call> Fn(
        &mut HostCallContext<'call>,
        &HostPathContext<ValueView<'call>>,
    ) -> Result<ValueView<'call>, HostError>
    + Send
    + Sync
    + 'static;

/// Preparation validates and reserves host resources without changing the target.
pub type HostPathPrepareWriteCallback = dyn for<'call> Fn(
        &mut HostCallContext<'call>,
        &HostPathContext<ValueView<'call>>,
        &HostPathMutationRecord<ValueView<'call>>,
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

pub type HostPathValidateCallback = dyn for<'call> Fn(
        &mut HostCallContext<'call>,
        &HostPathContext<ValueView<'call>>,
        HostPathOperation,
        Option<&ValueView<'call>>,
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
        read: impl for<'call> Fn(
            &mut HostCallContext<'call>,
            &HostPathContext<ValueView<'call>>,
        ) -> Result<ValueView<'call>, HostError>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        self.read = Some(Arc::new(read));
        self
    }

    pub fn with_prepare_write(
        mut self,
        prepare: impl for<'call> Fn(
            &mut HostCallContext<'call>,
            &HostPathContext<ValueView<'call>>,
            &HostPathMutationRecord<ValueView<'call>>,
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
        validate: impl for<'call> Fn(
            &mut HostCallContext<'call>,
            &HostPathContext<ValueView<'call>>,
            HostPathOperation,
            Option<&ValueView<'call>>,
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

fn apply_path_modify<'call>(
    op: BinaryOp,
    old_value: ValueView<'call>,
    rhs: ValueView<'call>,
) -> Result<ValueView<'call>, RuntimeError> {
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
pub(crate) struct HostFrameMark {
    frame_id: HostFrameId,
    epoch: BorrowEpoch,
}

enum HostFrameAccess<'scope> {
    Owned(HostFrameMark),
    Borrowed(&'scope HostFrameMark),
}

pub struct HostCallGuard<'runtime> {
    table: &'runtime HostBorrowTable,
    resources: Option<&'runtime ResourceState>,
    frame: Option<HostFrameAccess<'runtime>>,
}

impl HostCallGuard<'_> {
    pub fn frame_id(&self) -> HostFrameId {
        self.mark().frame_id
    }

    pub fn epoch(&self) -> BorrowEpoch {
        self.mark().epoch
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
        if token.frame_id != self.frame_id() {
            return Err(RuntimeError::expired_host_borrow(format!(
                "token frame {} does not match current frame {}",
                token.frame_id.index(),
                self.frame_id().index()
            )));
        }
        self.table.validate(token, required_kind)
    }
}

impl<'runtime> HostCallGuard<'runtime> {
    fn mark(&self) -> &HostFrameMark {
        match self.frame.as_ref().expect("live Host frame access") {
            HostFrameAccess::Owned(mark) => mark,
            HostFrameAccess::Borrowed(mark) => mark,
        }
    }

    pub(crate) fn into_mark(mut self) -> HostFrameMark {
        match self.frame.take().expect("live owned Host frame") {
            HostFrameAccess::Owned(mark) => mark,
            HostFrameAccess::Borrowed(_) => unreachable!("only owned admission can transfer"),
        }
    }

    pub(crate) fn borrow_mark(
        table: &'runtime HostBorrowTable,
        resources: &'runtime ResourceState,
        mark: &'runtime HostFrameMark,
    ) -> Self {
        Self {
            table,
            resources: Some(resources),
            frame: Some(HostFrameAccess::Borrowed(mark)),
        }
    }

    pub(crate) fn from_mark(
        table: &'runtime HostBorrowTable,
        resources: &'runtime ResourceState,
        mark: HostFrameMark,
    ) -> Self {
        Self {
            table,
            resources: Some(resources),
            frame: Some(HostFrameAccess::Owned(mark)),
        }
    }
}

impl Drop for HostCallGuard<'_> {
    fn drop(&mut self) {
        if let Some(HostFrameAccess::Owned(mark)) = self.frame.take() {
            self.table
                .leave_frame(mark.frame_id, mark.epoch, self.resources);
        }
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

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
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

    pub(crate) fn retained_bytes(&self) -> usize {
        self.message.capacity()
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
    pub(crate) fn new(
        runtime: &'a mut Runtime,
        args: &[RootedValue],
    ) -> Result<Self, RuntimeError> {
        Ok(Self {
            scope: runtime.host_scope(args)?,
        })
    }

    pub fn runtime(&self) -> &Runtime {
        self.scope.runtime()
    }

    pub fn runtime_mut(&mut self) -> &mut Runtime {
        self.scope.runtime_mut()
    }

    pub fn borrows(&self) -> HostCallGuard<'_> {
        self.scope.borrows()
    }

    /// Protect a complete callback input pack in this invocation's suffix.
    pub fn retain_temporaries(
        &mut self,
        values: &[ValueView<'_>],
    ) -> Result<ArgumentValues<'a>, RuntimeError> {
        self.scope.retain_values(values)
    }

    pub fn import(&mut self, value: &RootedValue) -> Result<ValueView<'a>, RuntimeError> {
        self.scope.import(value)
    }

    pub fn protect(&mut self, value: &ValueView<'_>) -> Result<ValueView<'a>, RuntimeError> {
        self.scope.protect(value)
    }

    pub fn retain(&mut self, value: &ValueView<'_>) -> Result<RootedValue, RuntimeError> {
        self.scope.retain(value)
    }

    /// Borrow a protected String without copying it or registering a Host root.
    /// Runtime execution is unavailable until the text callback returns. The
    /// borrowed text cannot escape:
    /// ```compile_fail
    /// use kagari_runtime::{host::HostCallContext, value::view::ValueView};
    /// fn escape<'a>(cx: &mut HostCallContext<'a>, value: &ValueView<'a>) -> &'a str {
    ///     cx.read_string(value, |text| text).unwrap()
    /// }
    /// ```
    pub fn read_string<R>(
        &mut self,
        value: &ValueView<'_>,
        read: impl for<'text> FnOnce(&'text str) -> R,
    ) -> Result<R, RuntimeError> {
        self.scope.read_string(value, read)
    }

    pub(crate) fn validate_arguments(
        &mut self,
        values: &[ValueView<'_>],
    ) -> Result<(), RuntimeError> {
        self.scope.validate_values(values)
    }

    pub(crate) fn validate_result(&mut self, value: &ValueView<'_>) -> Result<(), RuntimeError> {
        self.scope.validate_result(value)
    }

    /// Build a managed callback result through the canonical converter. Publish
    /// into this Host scope before the child conversion and its arguments retire.
    pub fn convert_value(
        &mut self,
        owner: &ExecutableModule,
        convert: impl for<'conversion> FnOnce(
            &mut ConversionContext<'conversion>,
        ) -> Result<ValueView<'conversion>, RuntimeError>,
    ) -> Result<ValueView<'a>, RuntimeError> {
        self.scope.execute(&[], |runtime, _, result| {
            let mut conversion = ConversionContext::new(runtime, owner)?;
            let value = convert(&mut conversion)?;
            result.publish(conversion.runtime_mut(), &value)
        })
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
    dirty_paths: Vec<HostPathMutationRecord<RootedValue>>,
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
    if matches!(
        error.kind(),
        RuntimeErrorKind::HostCallFailure | RuntimeErrorKind::ModuleValidation
    ) {
        RuntimeError::typed_path_validation(error.to_string())
    } else {
        error
    }
}

#[cfg(test)]
mod tests;
