use std::{cell::RefCell, collections::HashMap, fmt, sync::Arc};

use kagari_ir::bytecode::BinaryOp;

use crate::{
    error::RuntimeError,
    metadata::{
        AbiFingerprint, FieldInfo, FieldMetadataId, MethodInfo, PathAccess, TraitInfo, TypeId,
        TypeKind, TypeRegistration,
    },
    security::CapabilitySet,
    value::Value,
};

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
    object_id: HostObjectId,
    type_id: TypeId,
    schema_epoch: HostSchemaEpoch,
    abi_fingerprint: AbiFingerprint,
}

impl HostRootHandle {
    pub fn new(
        object_id: HostObjectId,
        type_id: TypeId,
        schema_epoch: HostSchemaEpoch,
        abi_fingerprint: AbiFingerprint,
    ) -> Self {
        Self {
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
        abi_fingerprint: AbiFingerprint,
    },
    Virtual {
        name: String,
        result_type: TypeId,
        access: PathAccess,
        abi_fingerprint: AbiFingerprint,
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
            }
            | Self::Index {
                abi_fingerprint, ..
            }
            | Self::Virtual {
                abi_fingerprint, ..
            } => *abi_fingerprint,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostPathDescriptorRegistration {
    pub root_type: TypeId,
    pub result_type: TypeId,
    pub segments: Vec<HostPathSegment>,
    pub access: PathAccess,
    pub schema_epoch: HostSchemaEpoch,
    pub abi_fingerprint: AbiFingerprint,
    pub capability_requirements: CapabilitySet,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostPathDescriptor {
    pub id: HostPathDescriptorId,
    pub root_type: TypeId,
    pub result_type: TypeId,
    pub segments: Vec<HostPathSegment>,
    pub dynamic_parameters: Vec<DynamicPathParameter>,
    pub access: PathAccess,
    pub schema_epoch: HostSchemaEpoch,
    pub abi_fingerprint: AbiFingerprint,
    pub capability_requirements: CapabilitySet,
}

impl HostPathDescriptor {
    fn from_registration(
        id: HostPathDescriptorId,
        registration: HostPathDescriptorRegistration,
    ) -> Result<Self, RuntimeError> {
        validate_path_access(registration.access, "path descriptor")?;
        let dynamic_parameters = collect_dynamic_parameters(&registration.segments)?;
        let Some(last_segment) = registration.segments.last() else {
            return Err(RuntimeError::typed_path_validation(
                "path descriptor must contain at least one segment",
            ));
        };
        if last_segment.result_type() != registration.result_type {
            return Err(RuntimeError::typed_path_validation(
                "path descriptor result type does not match its final segment",
            ));
        }
        for segment in &registration.segments {
            validate_path_access(segment.access(), "path segment")?;
            if !path_access_allows(segment.access(), registration.access) {
                return Err(RuntimeError::typed_path_validation(
                    "path descriptor access exceeds a segment access policy",
                ));
            }
        }
        Ok(Self {
            id,
            root_type: registration.root_type,
            result_type: registration.result_type,
            segments: registration.segments,
            dynamic_parameters,
            access: registration.access,
            schema_epoch: registration.schema_epoch,
            abi_fingerprint: registration.abi_fingerprint,
            capability_requirements: registration.capability_requirements,
        })
    }

    pub fn requires_dynamic_args(&self) -> bool {
        !self.dynamic_parameters.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct HostPathViewHandle {
    root: HostRootHandle,
    base: Option<Box<HostPathViewHandle>>,
    descriptor_id: HostPathDescriptorId,
    result_type: TypeId,
    access: PathAccess,
    schema_epoch: HostSchemaEpoch,
    dynamic_args: DynamicPathArguments,
}

impl HostPathViewHandle {
    fn new(
        root: HostRootHandle,
        base: Option<HostPathViewHandle>,
        descriptor: &HostPathDescriptor,
        dynamic_args: DynamicPathArguments,
    ) -> Self {
        Self {
            root,
            base: base.map(Box::new),
            descriptor_id: descriptor.id,
            result_type: descriptor.result_type,
            access: descriptor.access,
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
}

#[derive(Debug, Clone, PartialEq)]
pub struct HostPathContext {
    pub root: HostRootHandle,
    pub base_view: Option<HostPathViewHandle>,
    pub descriptor: HostPathDescriptor,
    pub dynamic_args: DynamicPathArguments,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HostPathMutationRecord {
    pub root: HostRootHandle,
    pub base_view: Option<HostPathViewHandle>,
    pub descriptor_id: HostPathDescriptorId,
    pub operation: HostPathOperation,
    pub dynamic_args: DynamicPathArguments,
    pub old_value: Option<Value>,
    pub new_value: Value,
}

pub type HostPathReadCallback =
    dyn Fn(&HostPathContext) -> Result<Value, HostError> + Send + Sync + 'static;
pub type HostPathWriteCallback =
    dyn Fn(&HostPathContext, Value) -> Result<(), HostError> + Send + Sync + 'static;
pub type HostPathValidateCallback = dyn Fn(&HostPathContext, HostPathOperation, Option<&Value>) -> Result<(), HostError>
    + Send
    + Sync
    + 'static;
pub type HostPathDirtyCallback =
    dyn Fn(&HostPathMutationRecord) -> Result<(), HostError> + Send + Sync + 'static;

#[derive(Clone, Default)]
pub struct HostPathAdapter {
    read: Option<Arc<HostPathReadCallback>>,
    write: Option<Arc<HostPathWriteCallback>>,
    validate: Option<Arc<HostPathValidateCallback>>,
    dirty: Option<Arc<HostPathDirtyCallback>>,
}

impl fmt::Debug for HostPathAdapter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostPathAdapter")
            .field("has_read", &self.read.is_some())
            .field("has_write", &self.write.is_some())
            .field("has_validate", &self.validate.is_some())
            .field("has_dirty", &self.dirty.is_some())
            .finish()
    }
}

impl HostPathAdapter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_read(
        mut self,
        read: impl Fn(&HostPathContext) -> Result<Value, HostError> + Send + Sync + 'static,
    ) -> Self {
        self.read = Some(Arc::new(read));
        self
    }

    pub fn with_write(
        mut self,
        write: impl Fn(&HostPathContext, Value) -> Result<(), HostError> + Send + Sync + 'static,
    ) -> Self {
        self.write = Some(Arc::new(write));
        self
    }

    pub fn with_validate(
        mut self,
        validate: impl Fn(&HostPathContext, HostPathOperation, Option<&Value>) -> Result<(), HostError>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        self.validate = Some(Arc::new(validate));
        self
    }

    pub fn with_dirty(
        mut self,
        dirty: impl Fn(&HostPathMutationRecord) -> Result<(), HostError> + Send + Sync + 'static,
    ) -> Self {
        self.dirty = Some(Arc::new(dirty));
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
        if !arg.value.is_storable() {
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
    match op {
        BinaryOp::Add => match (old_value, rhs) {
            (Value::I32(lhs), Value::I32(rhs)) => Ok(Value::I32(lhs + rhs)),
            (Value::F32(lhs), Value::F32(rhs)) => Ok(Value::F32(lhs + rhs)),
            _ => Err(RuntimeError::typed_path_validation(
                "path add modify expects matching numeric values",
            )),
        },
        BinaryOp::Sub => match (old_value, rhs) {
            (Value::I32(lhs), Value::I32(rhs)) => Ok(Value::I32(lhs - rhs)),
            (Value::F32(lhs), Value::F32(rhs)) => Ok(Value::F32(lhs - rhs)),
            _ => Err(RuntimeError::typed_path_validation(
                "path sub modify expects matching numeric values",
            )),
        },
        BinaryOp::Mul => match (old_value, rhs) {
            (Value::I32(lhs), Value::I32(rhs)) => Ok(Value::I32(lhs * rhs)),
            (Value::F32(lhs), Value::F32(rhs)) => Ok(Value::F32(lhs * rhs)),
            _ => Err(RuntimeError::typed_path_validation(
                "path mul modify expects matching numeric values",
            )),
        },
        BinaryOp::Div => match (old_value, rhs) {
            (Value::I32(lhs), Value::I32(rhs)) => Ok(Value::I32(lhs / rhs)),
            (Value::F32(lhs), Value::F32(rhs)) => Ok(Value::F32(lhs / rhs)),
            _ => Err(RuntimeError::typed_path_validation(
                "path div modify expects matching numeric values",
            )),
        },
        BinaryOp::Eq
        | BinaryOp::NotEq
        | BinaryOp::Lt
        | BinaryOp::Gt
        | BinaryOp::Le
        | BinaryOp::Ge => Err(RuntimeError::typed_path_validation(
            "path modify expects an arithmetic assignment operator",
        )),
    }
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
    next_path_descriptor_id: usize,
    functions: HashMap<String, HostFunction>,
    types: HashMap<TypeId, HostTypeInfo>,
    type_names: HashMap<String, TypeId>,
    roots: HashMap<HostObjectId, HostRootHandle>,
    path_descriptors: HashMap<HostPathDescriptorId, HostPathDescriptor>,
    path_adapters: HashMap<HostPathDescriptorId, HostPathAdapter>,
    dirty_paths: RefCell<Vec<HostPathMutationRecord>>,
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
        if info.ownership != HostTypeOwnership::HostRoot {
            return Err(RuntimeError::typed_path_validation(
                "host root type must use HostRoot ownership",
            ));
        }
        validate_path_access(info.path_access, "host root type")?;
        let root = HostRootHandle::new(object_id, type_id, schema_epoch, info.abi_fingerprint);
        self.roots.insert(object_id, root);
        Ok(root)
    }

    pub fn root(&self, object_id: HostObjectId) -> Option<HostRootHandle> {
        self.roots.get(&object_id).copied()
    }

    pub fn roots(&self) -> impl Iterator<Item = HostRootHandle> + '_ {
        self.roots.values().copied()
    }

    pub fn register_path_descriptor(
        &mut self,
        registration: HostPathDescriptorRegistration,
    ) -> Result<HostPathDescriptorId, RuntimeError> {
        let Some(root_type) = self.types.get(&registration.root_type) else {
            return Err(RuntimeError::typed_path_validation(
                "path descriptor root type is not registered",
            ));
        };
        if root_type.ownership != HostTypeOwnership::HostRoot {
            return Err(RuntimeError::typed_path_validation(
                "path descriptor root type must use HostRoot ownership",
            ));
        }
        if !path_access_allows(root_type.path_access, registration.access) {
            return Err(RuntimeError::typed_path_validation(
                "path descriptor access exceeds root type policy",
            ));
        }

        let id = HostPathDescriptorId::new(self.next_path_descriptor_id);
        let descriptor = HostPathDescriptor::from_registration(id, registration)?;
        self.next_path_descriptor_id += 1;
        self.path_descriptors.insert(id, descriptor);
        Ok(id)
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

    pub fn read_path(
        &self,
        root_or_view: &Value,
        descriptor_id: HostPathDescriptorId,
        dynamic_args: Vec<Value>,
    ) -> Result<Value, RuntimeError> {
        let context = self.resolve_path_context(
            root_or_view,
            descriptor_id,
            dynamic_args,
            HostPathOperation::Read,
        )?;
        let adapter = self.path_adapter(context.descriptor.id)?;
        self.validate_path_operation(&adapter, &context, HostPathOperation::Read, None)?;
        let read = adapter.read.as_ref().ok_or_else(|| {
            RuntimeError::typed_path_validation("path descriptor does not support reads")
        })?;
        read(&context).map_err(|error| {
            RuntimeError::typed_path_validation(format!(
                "host path read failed: {}",
                error.message()
            ))
        })
    }

    pub fn set_path(
        &self,
        root_or_view: &Value,
        descriptor_id: HostPathDescriptorId,
        dynamic_args: Vec<Value>,
        value: Value,
    ) -> Result<(), RuntimeError> {
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
        let adapter = self.path_adapter(context.descriptor.id)?;
        self.validate_path_operation(&adapter, &context, HostPathOperation::Set, Some(&value))?;
        let old_value = adapter.read.as_ref().and_then(|read| read(&context).ok());
        self.write_path_value(&adapter, &context, value.clone())?;
        self.mark_dirty(
            &adapter,
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

    pub fn modify_path(
        &self,
        root_or_view: &Value,
        descriptor_id: HostPathDescriptorId,
        dynamic_args: Vec<Value>,
        op: BinaryOp,
        value: Value,
    ) -> Result<Value, RuntimeError> {
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
        let adapter = self.path_adapter(context.descriptor.id)?;
        self.validate_path_operation(
            &adapter,
            &context,
            HostPathOperation::Modify(op),
            Some(&value),
        )?;
        let read = adapter.read.as_ref().ok_or_else(|| {
            RuntimeError::typed_path_validation("path descriptor does not support modify reads")
        })?;
        let old_value = read(&context).map_err(|error| {
            RuntimeError::typed_path_validation(format!(
                "host path modify read failed: {}",
                error.message()
            ))
        })?;
        let new_value = apply_path_modify(op, old_value.clone(), value)?;
        self.write_path_value(&adapter, &context, new_value.clone())?;
        self.mark_dirty(
            &adapter,
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

    pub fn clear_dirty_paths(&self) {
        self.dirty_paths.borrow_mut().clear();
    }

    fn resolve_path_context(
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

    fn path_adapter(
        &self,
        descriptor_id: HostPathDescriptorId,
    ) -> Result<HostPathAdapter, RuntimeError> {
        self.path_adapters
            .get(&descriptor_id)
            .cloned()
            .ok_or_else(|| RuntimeError::typed_path_validation("path descriptor has no adapter"))
    }

    fn validate_path_operation(
        &self,
        adapter: &HostPathAdapter,
        context: &HostPathContext,
        operation: HostPathOperation,
        value: Option<&Value>,
    ) -> Result<(), RuntimeError> {
        if let Some(validate) = &adapter.validate {
            validate(context, operation, value).map_err(|error| {
                RuntimeError::typed_path_validation(format!(
                    "host path validation failed: {}",
                    error.message()
                ))
            })?;
        }
        Ok(())
    }

    fn write_path_value(
        &self,
        adapter: &HostPathAdapter,
        context: &HostPathContext,
        value: Value,
    ) -> Result<(), RuntimeError> {
        let write = adapter.write.as_ref().ok_or_else(|| {
            RuntimeError::typed_path_validation("path descriptor does not support writes")
        })?;
        write(context, value).map_err(|error| {
            RuntimeError::typed_path_validation(format!(
                "host path write failed: {}",
                error.message()
            ))
        })
    }

    fn mark_dirty(
        &self,
        adapter: &HostPathAdapter,
        record: HostPathMutationRecord,
    ) -> Result<(), RuntimeError> {
        if let Some(dirty) = &adapter.dirty {
            dirty(&record).map_err(|error| {
                RuntimeError::typed_path_validation(format!(
                    "host path dirty hook failed: {}",
                    error.message()
                ))
            })?;
        }
        self.dirty_paths.borrow_mut().push(record);
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
