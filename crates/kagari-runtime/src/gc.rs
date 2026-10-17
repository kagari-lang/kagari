use crate::{
    closure::ClosureValueSnapshot,
    error::{RuntimeError, RuntimeErrorKind},
    error_trace::ErrorTrace,
    execution_metadata::{
        applications::ApplicationStore,
        environments::EnvironmentStore,
        groups::OperationGroupStore,
        interfaces::{InterfaceSnapshotId, InterfaceStore},
    },
    gc::{
        directory::{ObjectSlot, ObjectStorage, PayloadKind, PayloadLocation},
        fields::{StructStorageLayouts, StructView, StructViewMut},
        interfaces::InterfaceValueSnapshot,
        leases::{LeaseTable, OwnedLease},
        roots::{RootTable, RootedValue},
        storage::{Exclusive, HeapObject, Shared},
    },
    module::{EnumVariantRef, ModuleKey, StructLayoutRef},
    native::storage::{NativeObject, NativeStorage},
    resource::ResourceState,
    session::ExecutionPhase,
    value::{EnumTag, EnumValueSnapshot, InterfaceObjectId, Value},
    value_check::matches_type_in,
};
use kagari_abi::representation::ValueType;
use kagari_types::ty::Ty;
use std::{
    cell::{Cell, OnceCell, Ref, RefCell, RefMut},
    collections::HashSet,
    slice,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    time::Duration,
};

#[cfg(feature = "execution-diagnostics")]
mod accounting;
mod arrays;
mod fields;
mod future;
mod maps_sets;
mod native;
mod sequences;
mod task;

mod buffers;
mod capacity;
mod collection;
mod collector;
pub(crate) mod custom_keys;
mod directory;
pub(crate) mod interfaces;
mod iter;
pub(crate) mod leases;
pub mod mutations;
pub mod roots;
mod sequence_edit;
mod sequence_ops;
mod storage;
mod string_iter;
mod values;

#[derive(Debug, Clone, Copy)]
pub struct GcHeapConfig {
    /// Minimum combined heap-unit/executable-metadata threshold at execution safepoints.
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
    /// Live groups of receiver methods or concrete selected operations.
    pub operation_groups: usize,
    /// Live prepared method application records.
    pub method_applications: usize,
    /// Live interface view records, including cached parent views.
    pub interface_snapshots: usize,
    /// Live executable environment records.
    pub environments: usize,
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

/// Unrooted checked identity. Copies never retain objects or expose their addresses.
/// Reclamation retires a slot when its allocation generation cannot advance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HeapObjectId {
    owner: u32,
    slot: u32,
    generation: u32,
}

impl HeapObjectId {
    pub fn index(self) -> usize {
        self.slot as usize
    }

    pub fn generation(self) -> u32 {
        self.generation
    }

    // Both conversions follow allocation admission. They must never truncate a
    // foreign owner or an unrepresentable slot into a valid compact identity.
    fn new(owner: u64, slot: usize, generation: u32) -> Self {
        Self {
            owner: u32::try_from(owner).expect("admitted heap identity"),
            slot: u32::try_from(slot).expect("admitted heap slot"),
            generation,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GcCollection {
    /// Operation group records detached by this collection.
    pub reclaimed_operation_groups: usize,
    /// Prepared method application records detached by this collection.
    pub reclaimed_method_applications: usize,
    /// Interface view records detached by this collection.
    pub reclaimed_interface_snapshots: usize,
    /// Executable environments detached by this collection.
    pub reclaimed_environments: usize,
    /// Program members whose mutable instance storage was released in this pass.
    /// Immutable layout/code descriptors may remain in retained data values.
    pub reclaimed_modules: Vec<ModuleKey>,
    pub reclaimed_objects: usize,
    pub reclaimed_units: usize,
    pub live_objects: usize,
    pub pause: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GcObjectKind {
    String,
    Tuple,
    Range,
    HostRoot,
    HostPath,
    Ephemeral,
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

/// Keeps a mutation target alive and rejects conflicting callback writes.
#[must_use = "retain the guard until the mutation finishes"]
#[derive(Debug)]
pub(crate) struct CollectionMutation {
    _lease: OwnedLease,
    _root: RootedValue,
}

#[derive(Debug)]
pub struct GcHeap {
    owner: u64,
    config: GcHeapConfig,
    objects: RefCell<ObjectStorage>,
    struct_layouts: RefCell<StructStorageLayouts>,
    free: RefCell<Vec<usize>>,
    moved: RefCell<HashSet<HeapObjectId>>,
    roots: RefCell<RootTable>,
    pub(crate) environments: RefCell<EnvironmentStore>,
    pub(crate) interfaces: RefCell<InterfaceStore>,
    pub(crate) applications: RefCell<ApplicationStore>,
    pub(crate) operation_groups: RefCell<OperationGroupStore>,
    stats: RefCell<CollectorStats>,
    resources: ResourceState,
    next_collection: Cell<usize>,
    mutations: LeaseTable,
    key_lookups: LeaseTable,
    native_operations: LeaseTable,
    next_key_token: Cell<i64>,
    native_borrows: Cell<usize>,
    // Heap-local immutable codecs contain no object identity or loaded types.
    // Initialize only when used; descriptor construction cannot execute callbacks.
    sequence_storage: OnceCell<NativeStorage>,
    map_storage: OnceCell<NativeStorage>,
    set_storage: OnceCell<NativeStorage>,
    cursor_storage: OnceCell<NativeStorage>,
}

impl GcHeap {
    pub(crate) fn resources(&self) -> &ResourceState {
        &self.resources
    }

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

    pub fn new(config: GcHeapConfig, resources: ResourceState) -> Self {
        static NEXT_OWNER: AtomicU32 = AtomicU32::new(1);
        let owner = u64::from(
            NEXT_OWNER
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
                    next.checked_add(1)
                })
                .expect("heap identity exhausted"),
        );
        Self {
            owner,
            config,
            mutations: Default::default(),
            key_lookups: Default::default(),
            native_operations: Default::default(),
            next_key_token: Cell::new(0),
            native_borrows: Cell::new(0),
            sequence_storage: OnceCell::new(),
            map_storage: OnceCell::new(),
            set_storage: OnceCell::new(),
            cursor_storage: OnceCell::new(),
            objects: RefCell::new(ObjectStorage::default()),
            struct_layouts: RefCell::new(StructStorageLayouts::default()),
            free: RefCell::new(Vec::new()),
            moved: RefCell::new(HashSet::new()),
            roots: RefCell::new(RootTable::default()),
            environments: RefCell::new(EnvironmentStore::new(owner)),
            interfaces: RefCell::new(InterfaceStore::new(owner)),
            applications: RefCell::new(ApplicationStore::new(owner)),
            operation_groups: RefCell::new(OperationGroupStore::new(owner)),
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
            operation_groups: self.operation_groups.borrow().count(),
            method_applications: self.applications.borrow().count(),
            interface_snapshots: self.interfaces.borrow().count(),
            environments: self.environments.borrow().count(),
            current_heap_units: counters.current_heap_units,
            peak_heap_units: counters.peak_heap_units,
            allocated_objects: stats.allocated_objects,
            collections: stats.collections,
            reclaimed_objects: stats.reclaimed_objects,
            last_pause: stats.last_pause,
        }
    }

    /// Active persistent root groups and separately traced execution windows.
    pub fn active_roots(&self) -> usize {
        self.roots.borrow().active() + self.resources.frame_values.borrow().active_windows()
    }

    pub(crate) fn alloc_struct(
        &self,
        layout: StructLayoutRef,
        fields: Vec<Value>,
    ) -> Result<HeapObjectId, RuntimeError> {
        self.ensure_execution_allowed()?;
        let storage = self.prepare_struct_storage(layout, fields)?;
        self.alloc_object(HeapObject::Struct { storage })
    }

    pub(crate) fn alloc_enum(
        &self,
        tag: EnumTag,
        fields: Vec<Value>,
    ) -> Result<HeapObjectId, RuntimeError> {
        self.ensure_execution_allowed()?;
        if !tag.accepts_representations(&fields)
            || !fields.iter().all(|value| self.valid_payload(value))
            || matches!(&tag, EnumTag::Declared(layout)
                if !fields.iter().enumerate().all(|(slot, value)| layout.payload_type(slot).is_some_and(|(ty, environment)| matches_type_in(self, value, ty, layout.module(), environment))))
        {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "invalid heap target, index, or payload",
            ));
        }
        let trace = matches!(&tag, EnumTag::Declared(layout) if layout.variant().reports_failure)
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
            HeapObject::Enum(snapshot, trace) if matches!(&snapshot.tag, EnumTag::Declared(layout) if layout.variant().reports_failure) => {
                trace.clone()
            }
            _ => None,
        }
    }

    pub(crate) fn forward_enum_origin(
        &self,
        original: &Value,
        value: &Value,
    ) -> Result<HeapObjectId, RuntimeError> {
        self.ensure_execution_allowed()?;
        let (Value::Enum(original), Value::Enum(value)) = (original, value) else {
            return Err(RuntimeError::module_validation("enum origin carriers"));
        };
        let (snapshot, trace) = {
            let objects = self.objects.borrow();
            let Some(HeapObject::Enum(_, trace)) = self.object_ref(&objects, *original) else {
                return Err(RuntimeError::module_validation("origin carrier handle"));
            };
            let Some(HeapObject::Enum(snapshot, _)) = self.object_ref(&objects, *value) else {
                return Err(RuntimeError::module_validation("enum value handle"));
            };
            (snapshot.clone(), trace.clone())
        };
        self.alloc_object(HeapObject::Enum(snapshot, trace))
    }

    pub(crate) fn ensure_structure_mutable(&self, id: HeapObjectId) -> Result<(), RuntimeError> {
        self.ensure_key_mutable(id)?;
        Ok(())
    }

    pub(crate) fn enum_layout(&self, id: HeapObjectId) -> Option<EnumVariantRef> {
        self.enum_view(id).map(|snapshot| {
            let EnumTag::Declared(layout) = &snapshot.tag;
            layout.clone()
        })
    }

    /// Copy tag and fields for an owning inspection boundary. This copy does not
    /// root payload handles; retain their source root across allocation or reentry.
    /// Synchronous reads should use `enum_view` and release it before either boundary.
    pub fn enum_snapshot(&self, id: HeapObjectId) -> Option<EnumValueSnapshot> {
        self.enum_view(id).map(|view| (*view).clone())
    }

    pub fn object_kind(&self, id: HeapObjectId) -> Option<GcObjectKind> {
        let objects = self.objects.try_borrow().ok()?;
        let location = self.object_location(&objects, id)?;
        Some(match location.kind {
            PayloadKind::String => GcObjectKind::String,
            PayloadKind::Tuple => GcObjectKind::Tuple,
            PayloadKind::Range => GcObjectKind::Range,
            PayloadKind::HostRoot => GcObjectKind::HostRoot,
            PayloadKind::HostPath => GcObjectKind::HostPath,
            PayloadKind::Ephemeral => GcObjectKind::Ephemeral,
            PayloadKind::Native => match objects.native(location)?.ty {
                Ty::Array(..) => GcObjectKind::Array,
                Ty::Map { .. } => GcObjectKind::Map,
                Ty::Set(..) => GcObjectKind::Set,
                Ty::Iter(..) => GcObjectKind::Iter,
                _ => GcObjectKind::Native,
            },
            PayloadKind::Enum => GcObjectKind::Enum,
            PayloadKind::Struct => GcObjectKind::Struct,
            PayloadKind::Interface => GcObjectKind::Interface,
            PayloadKind::Closure => GcObjectKind::Closure,
            PayloadKind::Cell => GcObjectKind::Cell,
        })
    }

    pub(crate) fn alloc_interface(
        &self,
        snapshot: InterfaceSnapshotId,
    ) -> Result<InterfaceObjectId, RuntimeError> {
        self.ensure_execution_allowed()?;
        let view = self
            .interface_metadata(snapshot)
            .ok_or_else(|| RuntimeError::module_validation("invalid interface snapshot"))?;
        // Runtime checks the full receiver ABI before publication.
        let durable_host = matches!(
            (&view.data, &view.description.concrete_type),
            (Value::HostRoot(_), Ty::Host(_))
        );
        if !durable_host && !self.valid_payload(&view.data) {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "invalid interface receiver",
            ));
        }
        let method_count = view.description.methods.len();
        drop(view);
        self.alloc_object(HeapObject::Interface {
            snapshot,
            method_count,
        })
        .map(InterfaceObjectId)
    }

    pub(crate) fn alloc_closure(
        &self,
        snapshot: ClosureValueSnapshot,
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
        self.alloc_object(HeapObject::Closure { snapshot })
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
            Some(HeapObject::Cell { ty: actual, value }) if *actual == ty => Ok(*value),
            _ => Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "invalid capture cell",
            )),
        }
    }

    pub(crate) fn captured_cell_value(&self, id: HeapObjectId) -> Option<Value> {
        let objects = self.objects.borrow();
        match self.readable_object(&objects, id)? {
            HeapObject::Cell { ty, value } if value.has_representation(*ty) => Some(*value),
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
        let mut objects = self.objects_mut()?;
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

    pub(crate) fn closure_snapshot(
        &self,
        id: HeapObjectId,
    ) -> Option<Ref<'_, ClosureValueSnapshot>> {
        Ref::filter_map(self.objects.try_borrow().ok()?, |objects| {
            match self.readable_object(objects, id)? {
                HeapObject::Closure { snapshot } => Some(snapshot),
                _ => None,
            }
        })
        .ok()
    }

    pub(crate) fn interface_snapshot_id(
        &self,
        id: InterfaceObjectId,
    ) -> Option<InterfaceSnapshotId> {
        let objects = self.objects.try_borrow().ok()?;
        match self.readable_object(&objects, id.0)? {
            HeapObject::Interface { snapshot, .. } => Some(*snapshot),
            _ => None,
        }
    }

    pub(crate) fn interface_snapshot(
        &self,
        id: InterfaceObjectId,
    ) -> Option<Ref<'_, InterfaceValueSnapshot>> {
        self.interface_metadata(self.interface_snapshot_id(id)?)
    }

    pub fn trace_roots(&self) -> Option<Vec<HeapObjectId>> {
        self.trace_values(&self.root_snapshots()?)
    }

    pub fn trace_value(&self, value: &Value) -> Option<Vec<HeapObjectId>> {
        self.trace_values(slice::from_ref(value))
    }

    pub(crate) fn automatic_collection_enabled(&self) -> bool {
        self.config.collection_threshold.is_some()
    }

    pub(crate) fn collection_due(&self) -> bool {
        self.config.collection_threshold.is_some()
            && self
                .resources
                .counters()
                .current_heap_units
                .saturating_add(self.operation_groups.borrow().count())
                .saturating_add(self.applications.borrow().count())
                .saturating_add(self.interfaces.borrow().count())
                .saturating_add(self.environments.borrow().count())
                >= self.next_collection.get()
    }

    pub fn validate_value(&self, value: &Value) -> bool {
        let (id, expected) = match value {
            Value::Unit
            | Value::Bool(_)
            | Value::I32(_)
            | Value::I64(_)
            | Value::U64(_)
            | Value::F32(_)
            | Value::F64(_)
            | Value::RuntimeEphemeral(_) => return true,
            Value::Str(id) => (*id, Some(GcObjectKind::String)),
            Value::Tuple(id) => (*id, Some(GcObjectKind::Tuple)),
            Value::Range(id) => (*id, Some(GcObjectKind::Range)),
            Value::HostRoot(id) => (*id, Some(GcObjectKind::HostRoot)),
            Value::HostPathView(id) => (*id, Some(GcObjectKind::HostPath)),
            Value::Ephemeral(id) => (*id, Some(GcObjectKind::Ephemeral)),
            Value::Array(id) => (*id, Some(GcObjectKind::Array)),
            Value::Map(id) => (*id, Some(GcObjectKind::Map)),
            Value::Set(id) => (*id, Some(GcObjectKind::Set)),
            Value::Enum(id) => (*id, Some(GcObjectKind::Enum)),
            Value::Struct(id) => (*id, Some(GcObjectKind::Struct)),
            Value::Interface(id) => (id.0, Some(GcObjectKind::Interface)),
            Value::Closure(id) => (*id, Some(GcObjectKind::Closure)),
            Value::Cell(id) => (*id, Some(GcObjectKind::Cell)),
            Value::GcHandle(id) => (*id, None),
        };
        self.object_kind(id)
            .is_some_and(|actual| expected.is_none_or(|kind| actual == kind))
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
        if !value.is_default_heap_payload(self) {
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
            matches!(
                object,
                HeapObject::Enum(..)
                    | HeapObject::String(_)
                    | HeapObject::Tuple(_)
                    | HeapObject::Range(_)
            ) || objects.initialization_owners.get(&id) == Some(&owner)
        })
    }

    fn valid_payload(&self, value: &Value) -> bool {
        value.is_default_heap_payload(self) && self.validate_value(value)
    }

    // Scoped metadata views can borrow slots. Reject a conflicting write before
    // changing storage or accounting instead of panicking inside RefCell.
    fn objects_mut(&self) -> Result<RefMut<'_, ObjectStorage>, RuntimeError> {
        self.objects
            .try_borrow_mut()
            .map_err(|_| RuntimeError::module_validation("heap storage is borrowed"))
    }

    fn alloc_object(&self, object: HeapObject) -> Result<HeapObjectId, RuntimeError> {
        let growth = self.resources.prepare_heap_growth(object.units())?;
        let initialization_owner = self
            .resources
            .active_session()
            .filter(|session| session.options.phase == ExecutionPhase::CandidateInitialization)
            .map(|session| session.root.program_root().key());
        let mut objects = self.objects_mut()?;
        let mut free = self.free.borrow_mut();
        let index = free.last().copied().unwrap_or(objects.slots.len());
        let directory =
            u32::try_from(index).map_err(|_| self.resource_limit("heap object slots"))?;
        if index == objects.slots.len() {
            objects
                .slots
                .try_reserve(1)
                .map_err(|_| self.resource_limit("allocation capacity"))?;
        }
        if initialization_owner.is_some() {
            objects
                .initialization_owners
                .try_reserve(1)
                .map_err(|_| self.resource_limit("initialization owners"))?;
        }
        let location = objects
            .insert(directory, object)
            .map_err(|_| self.resource_limit("allocation capacity"))?;
        let slot = if index == objects.slots.len() {
            objects.slots.push(ObjectSlot {
                revision: 0,
                generation: 0,
                location: Some(location),
            });
            index
        } else {
            free.pop();
            objects.slots[index].revision = 0;
            objects.slots[index].location = Some(location);
            index
        };
        let id = HeapObjectId::new(self.owner, slot, objects.slots[slot].generation);
        if let Some(owner) = initialization_owner {
            objects.initialization_owners.insert(id, owner);
        }
        self.stats.borrow_mut().allocated_objects += 1;
        growth.commit();
        Ok(id)
    }

    fn object_location(
        &self,
        objects: &ObjectStorage,
        id: HeapObjectId,
    ) -> Option<PayloadLocation> {
        if u64::from(id.owner) != self.owner {
            return None;
        }
        let slot = objects.slots.get(id.index())?;
        (slot.generation == id.generation)
            .then_some(slot.location)
            .flatten()
    }

    fn readable_location(
        &self,
        objects: &ObjectStorage,
        id: HeapObjectId,
    ) -> Option<PayloadLocation> {
        let location = self.object_location(objects, id)?;
        if let Some(session) = self
            .resources
            .active_session()
            .filter(|session| session.options.phase == ExecutionPhase::CandidateInitialization)
            && objects.initialization_owners.get(&id) != Some(&session.root.program_root().key())
        {
            return None;
        }
        Some(location)
    }

    fn object_ref<'a>(
        &self,
        objects: &'a ObjectStorage,
        id: HeapObjectId,
    ) -> Option<HeapObject<Shared<'a>>> {
        Some(objects.get(self.object_location(objects, id)?))
    }

    // Script-visible reads are isolated; collector traversal uses object_ref directly.
    fn readable_object<'a>(
        &self,
        objects: &'a ObjectStorage,
        id: HeapObjectId,
    ) -> Option<HeapObject<Shared<'a>>> {
        Some(objects.get(self.readable_location(objects, id)?))
    }

    fn object_mut<'a>(
        &self,
        objects: &'a mut ObjectStorage,
        id: HeapObjectId,
    ) -> Option<HeapObject<Exclusive<'a>>> {
        let location = self.readable_location(objects, id)?;
        Some(objects.get_mut(location))
    }

    fn native_ref<'a>(
        &self,
        objects: &'a ObjectStorage,
        id: HeapObjectId,
    ) -> Option<&'a NativeObject> {
        objects.native(self.readable_location(objects, id)?)
    }

    fn native_mut<'a>(
        &self,
        objects: &'a mut ObjectStorage,
        id: HeapObjectId,
    ) -> Option<&'a mut NativeObject> {
        let location = self.readable_location(objects, id)?;
        objects.native_mut(location)
    }

    fn release_heap_units(&self, units: usize) {
        self.resources.release_heap_units(units);
    }

    fn with_struct<R>(&self, id: HeapObjectId, f: impl FnOnce(StructView<'_>) -> R) -> Option<R> {
        let objects = self.objects.borrow();
        Some(f(objects.structure(self.readable_location(&objects, id)?)?))
    }

    fn with_struct_mut<R>(
        &self,
        id: HeapObjectId,
        f: impl FnOnce(StructViewMut<'_>) -> R,
    ) -> Option<R> {
        let mut objects = self.objects_mut().ok()?;
        let location = self.readable_location(&objects, id)?;
        Some(f(objects.structure_mut(location)?))
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod array_bulk_tests;
