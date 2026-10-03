mod arrays;
mod maps_sets;
mod native;
use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    error_trace::ErrorTrace,
    frame::types::{TypeEnvironment, compatibility::TypeView},
    gc::interfaces::InterfaceValueSnapshot,
    module::{LoadedModule, ModuleKey, RetainedRuntimeProgram, StructLayoutRef},
    native::{
        hashed::{MapPayload, SetPayload},
        sequence::SequencePayload,
        storage::{NativeObject, NativeStorage},
    },
    numeric,
    resource::ResourceState,
    session::ExecutionPhase,
    value::{EnumTag, EnumValueSnapshot, InterfaceObjectId, StructValueField, Value},
};
use kagari_abi::{
    ids::FunctionRef,
    representation::ValueType,
    types::{AbiType, native::NativeStorageLayout},
};
#[cfg(test)]
use kagari_common::collection::CollectionAccess;
use std::{
    borrow::Cow,
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    rc::{Rc, Weak},
    slice,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

mod array_ops;
mod capacity;
pub(crate) mod custom_keys;
pub(crate) mod interfaces;
mod iter;
mod iteration;
pub mod mutations;
mod sequence_edit;
mod string_iter;

#[derive(Debug, Clone, Copy)]
pub struct GcHeapConfig {
    /// Minimum live-unit threshold for collection at execution safepoints.
    pub collection_threshold: Option<usize>,
}

impl Default for GcHeapConfig {
    fn default() -> Self {
        Self {
            collection_threshold: Some(1024),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GcHeapStats {
    pub current_heap_units: usize,
    pub peak_heap_units: usize,
    pub allocated_objects: usize,
    pub collections: u64,
    pub reclaimed_objects: usize,
    pub last_pause: Duration,
}

#[derive(Debug, Default)]
struct CollectorStats {
    allocated_objects: usize,
    collections: u64,
    reclaimed_objects: usize,
    last_pause: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HeapObjectId {
    owner: u64,
    slot: usize,
    generation: u64,
}

impl HeapObjectId {
    pub fn index(self) -> usize {
        self.slot
    }
    pub fn generation(self) -> u64 {
        self.generation
    }
}

/// Owning root for host retention. Clones share one root; the last drop releases it.
#[must_use = "retain this handle for as long as the host needs the value"]
#[derive(Debug, Clone)]
pub struct RootedValue {
    roots: RootSet,
}
impl RootedValue {
    pub fn value(&self) -> Value {
        self.roots.get(0).expect("single root")
    }
    pub fn set(&self, heap: &GcHeap, value: Value) -> Option<()> {
        if !value.is_storable() {
            return None;
        }
        self.roots.set(heap, 0, value)
    }
}

/// A registered set of execution slots. Values in it remain live until last drop.
#[must_use = "retain the registered slots until execution resources are released"]
#[derive(Debug, Clone)]
pub struct RootSet {
    owner: u64,
    values: Rc<RefCell<Vec<Value>>>,
}
impl PartialEq for RootSet {
    fn eq(&self, other: &Self) -> bool {
        self.owner == other.owner && Rc::ptr_eq(&self.values, &other.values)
    }
}
impl RootSet {
    pub(crate) fn contains_slot(&self, index: usize) -> bool {
        index < self.values.borrow().len()
    }
    pub fn get(&self, index: usize) -> Option<Value> {
        self.values.borrow().get(index).cloned()
    }
    pub(crate) fn with_value<R>(&self, index: usize, read: impl FnOnce(&Value) -> R) -> Option<R> {
        let values = self.values.try_borrow().ok()?;
        values.get(index).map(read)
    }
    pub fn set(&self, heap: &GcHeap, index: usize, value: Value) -> Option<()> {
        if self.owner != heap.owner || !heap.validate_value(&value) {
            return None;
        }
        *self.values.borrow_mut().get_mut(index)? = value;
        Some(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GcCollection {
    pub reclaimed_objects: usize,
    pub reclaimed_units: usize,
    pub live_objects: usize,
    pub pause: Duration,
}

#[derive(Debug)]
struct ObjectSlot {
    revision: u64,
    generation: u64,
    initialization_owner: Option<ModuleKey>,
    object: Option<HeapObject>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GcObjectKind {
    Native,
    Iter,
    Array,
    Map,
    Set,
    Enum,
    Struct,
    Interface,
    Closure,
    Cell,
}

#[derive(Debug, Clone)]
pub struct ClosureValueSnapshot {
    pub environment: Option<Rc<TypeEnvironment>>,
    pub implementation: LoadedModule,
    pub function: FunctionRef,
    pub captures: Vec<Value>,
}

impl ClosureValueSnapshot {
    pub fn physical_signature(&self) -> Result<(Cow<'_, [ValueType]>, ValueType), RuntimeError> {
        let function = self
            .implementation
            .bytecode
            .functions
            .get(self.function.index())
            .ok_or_else(|| RuntimeError::module_validation("closure function"))?;
        let parameters = function
            .metadata
            .params
            .get(self.captures.len()..)
            .ok_or_else(|| RuntimeError::module_validation("closure captures"))?;
        if function.metadata.return_type != ValueType::Generic
            && !parameters.contains(&ValueType::Generic)
        {
            return Ok((Cow::Borrowed(parameters), function.metadata.return_type));
        }
        let resolve = |physical, semantic: Option<&AbiType>| {
            if physical != ValueType::Generic {
                return Ok(physical);
            }
            let ty =
                semantic.ok_or_else(|| RuntimeError::module_validation("generic closure type"))?;
            let environment = self
                .environment
                .as_ref()
                .ok_or_else(|| RuntimeError::module_validation("generic closure environment"))?;
            Ok(environment.resolve(ty)?.representation())
        };
        let params = function
            .metadata
            .params
            .iter()
            .enumerate()
            .skip(self.captures.len())
            .map(|(index, ty)| resolve(*ty, function.metadata.semantic.params.get(&index)))
            .collect::<Result<_, _>>()?;
        Ok((
            Cow::Owned(params),
            resolve(
                function.metadata.return_type,
                function.metadata.semantic.result.as_ref(),
            )?,
        ))
    }

    pub(crate) fn matches_function(
        &self,
        params: &[AbiType],
        result: &AbiType,
        owner: &LoadedModule,
        environment: Option<&TypeEnvironment>,
    ) -> bool {
        let Some(function) = self
            .implementation
            .bytecode
            .functions
            .get(self.function.index())
        else {
            return false;
        };
        let compatible = |actual: &AbiType, expected: &AbiType| {
            TypeView::new(actual, &self.implementation, self.environment.as_deref())
                .compatible(TypeView::new(expected, owner, environment))
        };
        let captures = self.captures.len();
        function
            .metadata
            .params
            .get(captures..)
            .is_some_and(|suffix| suffix.len() == params.len())
            && params.iter().enumerate().all(|(index, expected)| {
                function
                    .metadata
                    .semantic
                    .params
                    .get(&(captures + index))
                    .is_some_and(|actual| compatible(actual, expected))
            })
            && function
                .metadata
                .semantic
                .result
                .as_ref()
                .is_some_and(|actual| compatible(actual, result))
    }
}

#[derive(Debug)]
enum HeapObject {
    Native(NativeObject),
    Enum(EnumValueSnapshot, Option<Arc<ErrorTrace>>),
    Struct {
        layout: StructLayoutRef,
        fields: Vec<Value>,
    },
    Interface {
        snapshot: Rc<InterfaceValueSnapshot>,
        _retention: RetainedRuntimeProgram,
    },
    Closure {
        snapshot: Rc<ClosureValueSnapshot>,
        _retention: RetainedRuntimeProgram,
    },
    Cell {
        ty: ValueType,
        value: Value,
    },
}

impl HeapObject {
    fn units(&self) -> usize {
        1 + match self {
            Self::Native(object) => object.units(),
            Self::Enum(value, _) => value.fields.len(),
            Self::Struct { fields, .. } => fields.len(),
            Self::Interface { snapshot, .. } => 1 + snapshot.methods.len(),
            Self::Closure { snapshot, .. } => 1 + snapshot.captures.len(),
            Self::Cell { .. } => 2,
        }
    }
}

/// Keeps the iterated collection alive and blocks structural writes through aliases.
#[must_use = "retain the guard until iteration finishes"]
#[derive(Debug)]
pub struct CollectionIteration {
    _children: Vec<CollectionIteration>,
    iter_loops: Vec<Rc<Cell<usize>>>,
    active: Rc<RefCell<HashMap<HeapObjectId, usize>>>,
    id: Option<HeapObjectId>,
    _root: RootedValue,
}
impl Drop for CollectionIteration {
    fn drop(&mut self) {
        for loops in &self.iter_loops {
            loops.set(loops.get() - 1);
        }
        let Some(id) = self.id else {
            return;
        };
        let mut active = self.active.borrow_mut();
        let count = active.get_mut(&id).expect("registered iteration");
        *count -= 1;
        if *count == 0 {
            active.remove(&id);
        }
    }
}

#[derive(Debug)]
pub struct GcHeap {
    owner: u64,
    config: GcHeapConfig,
    objects: RefCell<Vec<ObjectSlot>>,
    free: RefCell<Vec<usize>>,
    roots: RefCell<Vec<Weak<RefCell<Vec<Value>>>>>,
    stats: RefCell<CollectorStats>,
    resources: Rc<ResourceState>,
    next_collection: Cell<usize>,
    iterations: Rc<RefCell<HashMap<HeapObjectId, usize>>>,
    mutations: Rc<RefCell<HashMap<HeapObjectId, usize>>>,
    key_lookups: Rc<RefCell<HashMap<HeapObjectId, usize>>>,
    next_key_token: Cell<i64>,
    native_borrows: Cell<usize>,
    sequence_storage: NativeStorage,
    map_storage: NativeStorage,
    set_storage: NativeStorage,
    cursor_storage: NativeStorage,
}

impl GcHeap {
    pub(crate) fn resource_limit(&self, name: &'static str) -> RuntimeError {
        self.resources.limit(name)
    }
    pub(crate) fn commit_host_write(&self, commit: impl FnOnce()) -> Result<(), RuntimeError> {
        self.resources.commit_host_write(commit)
    }

    pub(crate) fn prepare_dirty_record(&self, current: usize) -> Result<(), RuntimeError> {
        self.resources.prepare_dirty_record(current)
    }

    pub(crate) fn ensure_execution_allowed(&self) -> Result<(), RuntimeError> {
        self.ensure_no_native_borrow()?;
        self.resources.ensure_execution_allowed()
    }
    pub fn new(config: GcHeapConfig, resources: Rc<ResourceState>) -> Self {
        static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);
        let owner = NEXT_OWNER
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
                next.checked_add(1)
            })
            .expect("heap identity exhausted");
        Self {
            owner,
            config,
            iterations: Default::default(),
            mutations: Default::default(),
            key_lookups: Default::default(),
            next_key_token: Cell::new(0),
            native_borrows: Cell::new(0),
            sequence_storage: NativeStorage::sequence(0),
            map_storage: NativeStorage::map(0, 1),
            set_storage: NativeStorage::set(0),
            cursor_storage: NativeStorage::provided_with_layout::<iter::NativeIter>(
                NativeStorageLayout::Iterator { item: 0 },
            ),
            objects: RefCell::new(Vec::new()),
            free: RefCell::new(Vec::new()),
            roots: RefCell::new(Vec::new()),
            stats: RefCell::new(CollectorStats::default()),
            resources,
            next_collection: Cell::new(config.collection_threshold.unwrap_or(usize::MAX).max(1)),
        }
    }

    pub fn config(&self) -> GcHeapConfig {
        self.config
    }

    pub fn allocated_objects(&self) -> usize {
        self.stats.borrow().allocated_objects
    }

    pub fn stats(&self) -> GcHeapStats {
        let stats = self.stats.borrow();
        let counters = self.resources.counters();
        GcHeapStats {
            current_heap_units: counters.current_heap_units,
            peak_heap_units: counters.peak_heap_units,
            allocated_objects: stats.allocated_objects,
            collections: stats.collections,
            reclaimed_objects: stats.reclaimed_objects,
            last_pause: stats.last_pause,
        }
    }

    pub fn active_roots(&self) -> usize {
        self.roots
            .borrow()
            .iter()
            .filter(|root| root.strong_count() > 0)
            .count()
    }

    pub(crate) fn alloc_struct(
        &self,
        layout: StructLayoutRef,
        fields: Vec<Value>,
    ) -> Result<HeapObjectId, RuntimeError> {
        self.ensure_execution_allowed()?;
        if fields.len() != layout.layout().fields.len()
            || !fields.iter().enumerate().all(|(slot, value)| {
                self.valid_payload(value)
                    && layout.field_type(slot).is_some_and(|(ty, environment)| {
                        self.matches_type_in(value, ty, layout.module(), environment)
                    })
            })
        {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "invalid heap target, index, or payload",
            ));
        }
        self.alloc_object(HeapObject::Struct { layout, fields })
    }

    pub(crate) fn alloc_enum(
        &self,
        tag: EnumTag,
        fields: Vec<Value>,
    ) -> Result<HeapObjectId, RuntimeError> {
        self.ensure_execution_allowed()?;
        if !tag.accepts_representations(&fields)
            || !fields.iter().all(|value| self.valid_payload(value))
            || matches!(&tag, crate::value::EnumTag::Declared(layout)
                if !fields.iter().enumerate().all(|(slot, value)| layout.payload_type(slot).is_some_and(|(ty, environment)| self.matches_type_in(value, ty, layout.module(), environment))))
        {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "invalid heap target, index, or payload",
            ));
        }
        let trace = matches!(tag, crate::value::EnumTag::ResultErr)
            .then(|| ErrorTrace::capture(&self.resources));
        self.alloc_object(HeapObject::Enum(EnumValueSnapshot { tag, fields }, trace))
    }

    /// Diagnostic-only metadata; never participates in equality or key hashing.
    pub fn result_error_trace(&self, value: &Value) -> Option<Arc<ErrorTrace>> {
        let Value::Enum(id) = value else {
            return None;
        };
        let objects = self.objects.borrow();
        match self.object_ref(&objects, *id)? {
            HeapObject::Enum(snapshot, trace) if snapshot.tag == EnumTag::ResultErr => {
                trace.clone()
            }
            _ => None,
        }
    }
    pub(crate) fn map_result_error(
        &self,
        original: &Value,
        error: Value,
    ) -> Result<HeapObjectId, RuntimeError> {
        self.ensure_execution_allowed()?;
        let trace = self.result_error_trace(original).ok_or_else(|| {
            RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "expected Result Err to preserve its origin",
            )
        })?;
        if !self.valid_payload(&error) {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "invalid error payload",
            ));
        }
        self.alloc_object(HeapObject::Enum(
            EnumValueSnapshot {
                tag: EnumTag::ResultErr,
                fields: vec![error],
            },
            Some(trace),
        ))
    }

    pub(crate) fn matches_abi(&self, value: &Value, ty: &AbiType, owner: &LoadedModule) -> bool {
        if let AbiType::Builtin(kind) = ty {
            return if kind.integer_layout().is_some() {
                numeric::read_integer(*kind, value).is_ok()
            } else {
                value.has_representation(ty.representation())
            };
        }
        let mut pending = vec![(value.clone(), ty)];
        while let Some((value, ty)) = pending.pop() {
            match (value, ty) {
                (value, AbiType::Builtin(kind)) if if kind.integer_layout().is_some() {
                    numeric::read_integer(*kind, &value).is_ok()
                } else { value.has_representation(ty.representation()) } => {},
                (Value::Range(value), AbiType::Range(_, _)) if value.matches(ty) => {},
                (Value::Closure(id), AbiType::Function { params, result }) => {
                    let Some(snapshot) = self.closure_snapshot(id) else { return false; };
                    if !snapshot.matches_function(params, result, owner, None) { return false; }
                },
                (Value::Tuple(values), AbiType::Tuple(types)) if values.len() == types.len() => {
                    pending.extend(values.into_iter().zip(types));
                },
                (Value::Struct(id), AbiType::Struct(expected)) => {
                    if !self.struct_layout(id).is_some_and(|layout| owner.find_struct_layout(expected).is_some_and(|current| layout.matches(&current))) { return false; }
                },
                (Value::Enum(id), AbiType::Enum(_)) => {
                    if !self.enum_snapshot(id).is_some_and(|value| matches!(value.tag, EnumTag::Declared(layout) if layout.matches_type(ty, owner, None))) { return false; }
                },
                (Value::Interface(id), AbiType::Trait(_)) => {
                    if !self.interface_snapshot(id).is_some_and(|value| value.matches_type(ty, owner, None)) { return false; }
                },
                (Value::GcHandle(id), AbiType::NativeObject(_)) => {
                    if !self.matches_native_type(id, ty, owner, None) { return false; }
                },
                (Value::GcHandle(id), AbiType::Iter(element)) => {
                    let objects=self.objects.borrow();
                    let valid = match self.readable_object(&objects, id) {
                        Some(HeapObject::Native(object)) if matches!(object.ty, AbiType::Iter(_)) => object.payload::<iter::NativeIter>().is_ok_and(|iter| iter.item_contract.matches(element, owner)),
                        _ => false,
                    };
                    if !valid { return false; }
                },
                (Value::Array(id), AbiType::Array(element, _)) => {
                    let objects = self.objects.borrow();
                    let Some(HeapObject::Native(object)) = self.readable_object(&objects, id) else { return false; };
                    if !matches!(object.ty, AbiType::Array(..)) || !object.payload::<SequencePayload>().is_ok_and(|payload| payload.contract.matches(element, owner)) { return false; }
                },
                (Value::Map(id), AbiType::Map { key, value ,..}) => {
                    let objects = self.objects.borrow();
                    let Some(HeapObject::Native(object)) = self.readable_object(&objects, id) else { return false; };
                    if !matches!(object.ty, AbiType::Map { .. }) || !object.payload::<MapPayload>().is_ok_and(|payload| payload.key.matches(key, owner) && payload.value.matches(value, owner)) { return false; }
                },
                (Value::Set(id), AbiType::Set(element, _)) => {
                    let objects = self.objects.borrow();
                    let Some(HeapObject::Native(object)) = self.readable_object(&objects, id) else { return false; };
                    if !matches!(object.ty, AbiType::Set(..)) || !object.payload::<SetPayload>().is_ok_and(|payload| payload.element.matches(element, owner)) { return false; }
                },
                (Value::Enum(id), AbiType::StandardEnum { kind, args }) => {
                    let Some(snapshot) = self.enum_snapshot(id) else { return false; };
                    let Some(payload) = snapshot.tag.standard_payload(*kind) else { return false; };
                    let Some(index) = payload else {
                        if !snapshot.fields.is_empty() { return false; }
                        continue;
                    };
                    if snapshot.fields.len() != 1 { return false; }
                    let Some(ty) = args.get(index) else { return false; };
                    pending.extend(snapshot.fields.into_iter().map(|value| (value, ty)));
                },
                _ => return false,
            }
        }
        true
    }

    pub(crate) fn ensure_structure_mutable(&self, id: HeapObjectId) -> Result<(), RuntimeError> {
        self.ensure_key_mutable(id)?;
        if self.iterations.borrow().contains_key(&id) {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "structural modification during iteration",
            ));
        }
        Ok(())
    }

    pub fn struct_layout(&self, id: HeapObjectId) -> Option<StructLayoutRef> {
        self.with_struct(id, |layout, _| layout.clone())
    }
    pub fn struct_name(&self, id: HeapObjectId) -> Option<String> {
        self.with_struct(id, |layout, _| layout.layout().name().to_owned())
    }
    pub fn enum_snapshot(&self, id: HeapObjectId) -> Option<EnumValueSnapshot> {
        self.with_enum(id, Clone::clone)
    }
    pub fn struct_snapshot(&self, id: HeapObjectId) -> Option<(String, Vec<StructValueField>)> {
        self.with_struct(id, |layout, fields| {
            (
                layout.layout().name().to_owned(),
                fields
                    .iter()
                    .zip(&layout.layout().fields)
                    .map(|(value, field)| StructValueField {
                        name: field.name.clone(),
                        value: value.clone(),
                    })
                    .collect(),
            )
        })
    }
    pub fn struct_get_slot(
        &self,
        id: HeapObjectId,
        expected: &StructLayoutRef,
        slot: usize,
    ) -> Option<Value> {
        self.with_struct(id, |layout, fields| {
            if !layout.matches(expected) {
                return None;
            }
            fields.get(slot).cloned()
        })
        .flatten()
    }
    pub fn struct_set_slot(
        &self,
        id: HeapObjectId,
        expected: &StructLayoutRef,
        slot: usize,
        next_value: Value,
    ) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        if !self.valid_payload(&next_value) {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "invalid heap payload",
            ));
        }
        let field =
            expected.layout().fields.get(slot).ok_or_else(|| {
                RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid field slot")
            })?;
        if !field.mutable
            || !expected.field_type(slot).is_some_and(|(ty, environment)| {
                self.matches_type_in(&next_value, ty, expected.module(), environment)
            })
        {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "field is read-only or value has the wrong concrete type",
            ));
        }
        self.with_struct_mut(id, |layout, fields| {
            if !layout.matches(expected) {
                return Err(RuntimeError::new(
                    RuntimeErrorKind::ScriptTrap,
                    "struct layout mismatch",
                ));
            }
            let target = fields.get_mut(slot).ok_or_else(|| {
                self.resources
                    .quarantine("struct storage does not match its layout")
            })?;
            *target = next_value;
            Ok(())
        })
        .ok_or_else(|| RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid heap target"))?
    }

    pub fn object_kind(&self, id: HeapObjectId) -> Option<GcObjectKind> {
        let objects = self.objects.borrow();
        match self.object_ref(&objects, id)? {
            HeapObject::Native(object) => Some(match object.ty {
                AbiType::Array(..) => GcObjectKind::Array,
                AbiType::Map { .. } => GcObjectKind::Map,
                AbiType::Set(..) => GcObjectKind::Set,
                AbiType::Iter(..) => GcObjectKind::Iter,
                _ => GcObjectKind::Native,
            }),
            HeapObject::Enum(..) => Some(GcObjectKind::Enum),
            HeapObject::Struct { .. } => Some(GcObjectKind::Struct),
            HeapObject::Interface { .. } => Some(GcObjectKind::Interface),
            HeapObject::Closure { .. } => Some(GcObjectKind::Closure),
            HeapObject::Cell { .. } => Some(GcObjectKind::Cell),
        }
    }

    pub(crate) fn alloc_interface(
        &self,
        snapshot: InterfaceValueSnapshot,
        retention: RetainedRuntimeProgram,
    ) -> Result<InterfaceObjectId, RuntimeError> {
        self.ensure_execution_allowed()?;
        // Runtime::make_interface checks the full receiver ABI, including host
        // registry ownership/schema, before entering this allocation boundary.
        let durable_host = matches!(
            (&snapshot.data, &snapshot.concrete_type),
            (Value::HostRoot(_), kagari_abi::types::AbiType::Host(_))
        );
        if !durable_host && !self.valid_payload(&snapshot.data) {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "invalid interface receiver",
            ));
        }
        self.alloc_object(HeapObject::Interface {
            snapshot: Rc::new(snapshot),
            _retention: retention,
        })
        .map(InterfaceObjectId)
    }

    pub(crate) fn alloc_closure(
        &self,
        snapshot: ClosureValueSnapshot,
        retention: RetainedRuntimeProgram,
    ) -> Result<HeapObjectId, RuntimeError> {
        self.ensure_execution_allowed()?;
        if !snapshot
            .captures
            .iter()
            .all(|value| self.valid_payload(value))
        {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "invalid closure capture",
            ));
        }
        self.alloc_object(HeapObject::Closure {
            snapshot: Rc::new(snapshot),
            _retention: retention,
        })
    }

    pub(crate) fn alloc_cell(
        &self,
        ty: ValueType,
        value: Value,
    ) -> Result<HeapObjectId, RuntimeError> {
        self.ensure_execution_allowed()?;
        if !self.valid_payload(&value) || !value.has_representation(ty) {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "invalid cell value",
            ));
        }
        self.alloc_object(HeapObject::Cell { ty, value })
    }

    pub(crate) fn cell_get(&self, id: HeapObjectId, ty: ValueType) -> Result<Value, RuntimeError> {
        let objects = self.objects.borrow();
        match self.readable_object(&objects, id) {
            Some(HeapObject::Cell { ty: actual, value }) if *actual == ty => Ok(value.clone()),
            _ => Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "invalid capture cell",
            )),
        }
    }

    pub(crate) fn captured_cell_value(&self, id: HeapObjectId) -> Option<Value> {
        let objects = self.objects.borrow();
        match self.readable_object(&objects, id)? {
            HeapObject::Cell { ty, value } if value.has_representation(*ty) => Some(value.clone()),
            _ => None,
        }
    }

    pub(crate) fn cell_set(
        &self,
        id: HeapObjectId,
        ty: ValueType,
        value: Value,
    ) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        if !self.valid_payload(&value) || !value.has_representation(ty) {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "invalid cell value",
            ));
        }
        let mut objects = self.objects.borrow_mut();
        match self.object_mut(&mut objects, id) {
            Some(HeapObject::Cell {
                ty: actual,
                value: current,
            }) if *actual == ty => {
                *current = value;
                Ok(())
            }
            _ => Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "invalid capture cell",
            )),
        }
    }

    pub(crate) fn closure_snapshot(&self, id: HeapObjectId) -> Option<Rc<ClosureValueSnapshot>> {
        let objects = self.objects.borrow();
        match self.readable_object(&objects, id)? {
            HeapObject::Closure { snapshot, .. } => Some(snapshot.clone()),
            _ => None,
        }
    }

    pub(crate) fn interface_snapshot(
        &self,
        id: InterfaceObjectId,
    ) -> Option<Rc<InterfaceValueSnapshot>> {
        let objects = self.objects.borrow();
        match self.readable_object(&objects, id.0)? {
            HeapObject::Interface { snapshot, .. } => Some(snapshot.clone()),
            _ => None,
        }
    }

    pub fn root_value(&self, value: Value) -> Option<RootedValue> {
        if !value.is_storable() {
            return None;
        }
        Some(RootedValue {
            roots: self.root_execution_values(vec![value])?,
        })
    }

    pub fn root_execution_values(&self, values: Vec<Value>) -> Option<RootSet> {
        self.ensure_execution_allowed().ok()?;
        if !values.iter().all(|value| self.validate_value(value)) {
            return None;
        }
        let values = Rc::new(RefCell::new(values));
        let mut roots = self.roots.borrow_mut();
        roots.retain(|root| root.strong_count() > 0);
        roots.push(Rc::downgrade(&values));
        Some(RootSet {
            owner: self.owner,
            values,
        })
    }

    pub fn trace_roots(&self) -> Option<Vec<HeapObjectId>> {
        self.trace_values(&self.root_snapshots())
    }

    pub fn trace_value(&self, value: &Value) -> Option<Vec<HeapObjectId>> {
        self.trace_values(slice::from_ref(value))
    }

    fn root_snapshots(&self) -> Vec<Value> {
        self.roots
            .borrow()
            .iter()
            .filter_map(Weak::upgrade)
            .flat_map(|root| root.borrow().clone())
            .collect()
    }

    pub fn collection_due(&self) -> bool {
        self.config.collection_threshold.is_some()
            && self.resources.counters().current_heap_units >= self.next_collection.get()
    }

    /// Stop-the-world, nonmoving collection. Additional roots belong to runtime
    /// module state; registered host/frame roots are always included.
    pub(crate) fn collect(&self, additional_roots: &[Value]) -> Option<GcCollection> {
        let started = Instant::now();
        let mut values = self.root_snapshots();
        values.extend_from_slice(additional_roots);
        let live = self
            .trace_values(&values)?
            .into_iter()
            .collect::<HashSet<_>>();
        let mut reclaimed_objects = 0;
        let mut reclaimed_units = 0;
        let mut objects = self.objects.borrow_mut();
        let mut free = self.free.borrow_mut();
        for (index, slot) in objects.iter_mut().enumerate() {
            let id = HeapObjectId {
                owner: self.owner,
                slot: index,
                generation: slot.generation,
            };
            if !live.contains(&id)
                && let Some(object) = slot.object.take()
            {
                reclaimed_objects += 1;
                reclaimed_units += object.units();
                if let Some(generation) = slot.generation.checked_add(1) {
                    slot.generation = generation;
                    free.push(index);
                }
            }
        }
        drop(objects);
        self.release_heap_units(reclaimed_units);
        self.roots
            .borrow_mut()
            .retain(|root| root.strong_count() > 0);
        let pause = started.elapsed();
        let mut stats = self.stats.borrow_mut();
        stats.collections += 1;
        stats.reclaimed_objects += reclaimed_objects;
        stats.allocated_objects -= reclaimed_objects;
        stats.last_pause = pause;
        self.next_collection.set(
            self.resources
                .counters()
                .current_heap_units
                .saturating_mul(2)
                .max(self.config.collection_threshold.unwrap_or(usize::MAX))
                .max(1),
        );
        Some(GcCollection {
            reclaimed_objects,
            reclaimed_units,
            live_objects: live.len(),
            pause,
        })
    }

    pub fn validate_value(&self, value: &Value) -> bool {
        if matches!(
            value,
            Value::Unit
                | Value::Bool(_)
                | Value::I32(_)
                | Value::I64(_)
                | Value::U64(_)
                | Value::F32(_)
                | Value::F64(_)
                | Value::Str(_)
                | Value::HostRoot(_)
                | Value::Ephemeral(_)
        ) {
            return true;
        }
        let mut pending = vec![value];
        while let Some(value) = pending.pop() {
            let expected = match value {
                Value::HostPathView(view) => {
                    pending.extend(view.dynamic_args().as_slice().iter().map(|arg| &arg.value));
                    continue;
                }
                Value::Tuple(elements) => {
                    pending.extend(elements);
                    continue;
                }
                Value::Array(id) => (*id, Some(GcObjectKind::Array)),
                Value::Map(id) => (*id, Some(GcObjectKind::Map)),
                Value::Set(id) => (*id, Some(GcObjectKind::Set)),
                Value::Enum(id) => (*id, Some(GcObjectKind::Enum)),
                Value::Struct(id) => (*id, Some(GcObjectKind::Struct)),
                Value::Interface(id) => (id.0, Some(GcObjectKind::Interface)),
                Value::Closure(id) => (*id, Some(GcObjectKind::Closure)),
                Value::Cell(id) => (*id, Some(GcObjectKind::Cell)),
                Value::GcHandle(id) => (*id, None),
                _ => continue,
            };
            let Some(actual) = self.object_kind(expected.0) else {
                return false;
            };
            if expected.1.is_some_and(|kind| actual != kind) {
                return false;
            }
        }
        true
    }

    pub(crate) fn validate_candidate_value(&self, value: &Value) -> bool {
        let Some(session) = self
            .resources
            .active_session()
            .filter(|session| session.options.phase == ExecutionPhase::CandidateInitialization)
        else {
            return true;
        };
        self.validate_candidate_value_for(session.root.program_root().key(), value)
    }

    pub(crate) fn validate_candidate_value_for(&self, owner: ModuleKey, value: &Value) -> bool {
        if !value.is_default_heap_payload() {
            return false;
        }
        let Some(references) = self.trace_value(value) else {
            return false;
        };
        let objects = self.objects.borrow();
        references.into_iter().all(|id| {
            let Some(object) = self.object_ref(&objects, id) else {
                return false;
            };
            matches!(object, HeapObject::Enum(..))
                || objects[id.slot].initialization_owner == Some(owner)
        })
    }

    fn valid_payload(&self, value: &Value) -> bool {
        value.is_default_heap_payload() && self.validate_value(value)
    }

    fn alloc_object(&self, object: HeapObject) -> Result<HeapObjectId, RuntimeError> {
        let growth = self.resources.prepare_heap_growth(object.units())?;
        let initialization_owner = self
            .resources
            .active_session()
            .filter(|session| session.options.phase == ExecutionPhase::CandidateInitialization)
            .map(|session| session.root.program_root().key());
        let mut objects = self.objects.borrow_mut();
        let slot = if let Some(index) = self.free.borrow_mut().pop() {
            objects[index].revision = 0;
            objects[index].object = Some(object);
            objects[index].initialization_owner = initialization_owner;
            index
        } else {
            objects
                .try_reserve(1)
                .map_err(|_| self.resource_limit("allocation capacity"))?;
            let index = objects.len();
            objects.push(ObjectSlot {
                revision: 0,
                generation: 0,
                initialization_owner,
                object: Some(object),
            });
            index
        };
        self.stats.borrow_mut().allocated_objects += 1;
        growth.commit();
        Ok(HeapObjectId {
            owner: self.owner,
            slot,
            generation: objects[slot].generation,
        })
    }

    fn object_ref<'a>(
        &self,
        objects: &'a [ObjectSlot],
        id: HeapObjectId,
    ) -> Option<&'a HeapObject> {
        if id.owner != self.owner {
            return None;
        }
        let slot = objects.get(id.slot)?;
        if slot.generation != id.generation {
            return None;
        }
        slot.object.as_ref()
    }
    // Script-visible reads are isolated; collector traversal uses object_ref directly.
    fn readable_object<'a>(
        &self,
        objects: &'a [ObjectSlot],
        id: HeapObjectId,
    ) -> Option<&'a HeapObject> {
        let object = self.object_ref(objects, id)?;
        if let Some(session) = self
            .resources
            .active_session()
            .filter(|session| session.options.phase == ExecutionPhase::CandidateInitialization)
            && objects[id.slot].initialization_owner != Some(session.root.program_root().key())
        {
            return None;
        }
        Some(object)
    }

    fn object_mut<'a>(
        &self,
        objects: &'a mut [ObjectSlot],
        id: HeapObjectId,
    ) -> Option<&'a mut HeapObject> {
        if id.owner != self.owner {
            return None;
        }
        let slot = objects.get_mut(id.slot)?;
        if slot.generation != id.generation {
            return None;
        }
        if let Some(session) = self
            .resources
            .active_session()
            .filter(|session| session.options.phase == ExecutionPhase::CandidateInitialization)
            && slot.initialization_owner != Some(session.root.program_root().key())
        {
            return None;
        }
        slot.object.as_mut()
    }

    fn release_heap_units(&self, units: usize) {
        self.resources.release_heap_units(units);
    }

    fn trace_values(&self, values: &[Value]) -> Option<Vec<HeapObjectId>> {
        if !values.iter().all(|value| self.validate_value(value)) {
            return None;
        }
        let objects = self.objects.borrow();
        let mut seen = HashSet::new();
        let mut traced = Vec::new();
        let mut pending = values.iter().rev().collect::<Vec<_>>();
        while let Some(value) = pending.pop() {
            let id = match value {
                Value::HostPathView(view) => {
                    pending.extend(
                        view.dynamic_args()
                            .as_slice()
                            .iter()
                            .rev()
                            .map(|arg| &arg.value),
                    );
                    continue;
                }
                Value::Tuple(elements) => {
                    pending.extend(elements.iter().rev());
                    continue;
                }
                Value::Array(id)
                | Value::Map(id)
                | Value::Set(id)
                | Value::Enum(id)
                | Value::Struct(id)
                | Value::GcHandle(id) => *id,
                Value::Interface(id) => id.0,
                Value::Closure(id) => *id,
                Value::Cell(id) => *id,
                _ => continue,
            };
            let object = self.object_ref(&objects, id)?;
            if !seen.insert(id) {
                continue;
            }
            traced.push(id);
            match object {
                HeapObject::Native(object) => object.trace(&mut |value| pending.push(value)),
                HeapObject::Enum(snapshot, _) => pending.extend(snapshot.fields.iter().rev()),
                HeapObject::Struct { fields, .. } => pending.extend(fields.iter().rev()),
                HeapObject::Interface { snapshot, .. } => pending.push(&snapshot.data),
                HeapObject::Closure { snapshot, .. } => {
                    pending.extend(snapshot.captures.iter().rev())
                }
                HeapObject::Cell { value, .. } => pending.push(value),
            }
        }
        Some(traced)
    }

    fn with_enum<R>(&self, id: HeapObjectId, f: impl FnOnce(&EnumValueSnapshot) -> R) -> Option<R> {
        let objects = self.objects.borrow();
        match self.object_ref(&objects, id)? {
            HeapObject::Enum(snapshot, _) => Some(f(snapshot)),
            HeapObject::Native(_)
            | HeapObject::Struct { .. }
            | HeapObject::Interface { .. }
            | HeapObject::Closure { .. }
            | HeapObject::Cell { .. } => None,
        }
    }

    fn with_struct<R>(
        &self,
        id: HeapObjectId,
        f: impl FnOnce(&StructLayoutRef, &Vec<Value>) -> R,
    ) -> Option<R> {
        let objects = self.objects.borrow();
        match self.readable_object(&objects, id)? {
            HeapObject::Struct { layout, fields } => Some(f(layout, fields)),
            HeapObject::Native(_)
            | HeapObject::Enum(..)
            | HeapObject::Interface { .. }
            | HeapObject::Closure { .. }
            | HeapObject::Cell { .. } => None,
        }
    }

    fn with_struct_mut<R>(
        &self,
        id: HeapObjectId,
        f: impl FnOnce(&StructLayoutRef, &mut Vec<Value>) -> R,
    ) -> Option<R> {
        let mut objects = self.objects.borrow_mut();
        match self.object_mut(&mut objects, id)? {
            HeapObject::Struct { layout, fields } => Some(f(layout, fields)),
            HeapObject::Native(_)
            | HeapObject::Enum(..)
            | HeapObject::Interface { .. }
            | HeapObject::Closure { .. }
            | HeapObject::Cell { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod array_bulk_tests;
