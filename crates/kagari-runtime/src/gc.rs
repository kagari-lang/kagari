pub(crate) mod array_storage;
pub(crate) mod weak_cache;

use crate::{
    closure::ClosureValueSnapshot,
    error::{RuntimeError, RuntimeErrorKind},
    error_trace::ErrorTrace,
    execution_metadata::{
        MetadataEdge,
        applications::ApplicationStore,
        call_contracts::ScopedInterfaceCall,
        environments::EnvironmentStore,
        groups::OperationGroupStore,
        interfaces::{InterfaceSnapshotId, InterfaceStore},
        scan_records::{
            InterfaceCallScanId, InterfaceDescriptionScanId, NativeApplicationScanId,
            SharedScanStore,
        },
    },
    gc::{
        collection::CollectionCycle,
        directory::{ObjectStorage, PayloadKind, PayloadLocation, ResolvedDirect},
        fields::{StructStorageLayouts, StructView, StructViewMut},
        graph::{GraphIdentity, GraphState, driver::ActiveScan, marks::MarkRef},
        interfaces::{InterfaceDescription, InterfaceValueSnapshot},
        leases::LeaseTable,
        reference::GcRef,
        roots::RootTable,
        storage::{Exclusive, HeapObject, Shared},
    },
    module::{EnumVariantRef, ModuleKey, ModuleStore, StructLayoutRef},
    native::{
        application::NativeApplication,
        storage::{NativeObject, NativeStorage, scan::ScanWorkspace},
    },
    resource::{HeapCharge, ResourceState},
    session::ExecutionPhase,
    value::{EnumTag, EnumValueSnapshot, InterfaceObjectId, Value},
    value_check::matches_type_in,
};
use kagari_abi::representation::ValueType;
use kagari_types::ty::Ty;
use std::{
    cell::{Cell, OnceCell, Ref, RefCell, RefMut},
    slice,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    time::Duration,
};

#[cfg(feature = "execution-diagnostics")]
mod accounting;
mod array_access;
mod arrays;
pub(crate) mod fields;
mod future;
mod native;
#[cfg(test)]
pub(crate) mod retention_probe;
mod task;

mod buffers;
mod collection;
mod collector;
mod directory;
pub(crate) mod graph;
pub mod inspection;
pub(crate) mod interfaces;
mod iter;
pub(crate) mod leases;
pub mod metrics;
mod object_header;
pub(crate) mod publication;
pub mod reference;
pub mod roots;
mod storage;
mod string_iter;
mod values;

#[derive(Debug, Clone, Copy)]
pub struct GcHeapConfig {
    /// Minimum combined heap-unit/executable-metadata threshold at execution safepoints.
    pub collection_threshold: Option<usize>,
    /// Minimum attributed backing-byte pressure for automatic collection.
    /// Independent of logical units; collection_threshold=None disables both.
    pub retained_byte_threshold: usize,
}

impl Default for GcHeapConfig {
    fn default() -> Self {
        Self {
            collection_threshold: Some(1024),
            // A modest 64-KiB batch avoids collecting for each small reserve while
            // admitting large empty buffers. This is a policy floor, not a tuned optimum.
            retained_byte_threshold: 64 * 1024,
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
    /// Known backing capacity plus declared native pressure, not total heap/RSS.
    pub current_retained_bytes: u128,
    pub peak_retained_bytes: u128,
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
    observation: Option<Box<metrics::ObservationState>>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
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
    pub reclaimed_retained_bytes: u128,
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
    Enum,
    Struct,
    Interface,
    Closure,
    Cell,
}

#[derive(Debug)]
pub struct GcHeap {
    owner: u64,
    config: GcHeapConfig,
    objects: RefCell<ObjectStorage>,
    struct_layouts: RefCell<StructStorageLayouts>,

    roots: RefCell<RootTable>,
    pub(crate) environments: RefCell<EnvironmentStore>,
    pub(crate) interfaces: RefCell<InterfaceStore>,
    pub(crate) applications: RefCell<ApplicationStore>,
    pub(crate) operation_groups: RefCell<OperationGroupStore>,
    stats: RefCell<CollectorStats>,
    resources: ResourceState,
    next_collection: Cell<usize>,
    next_byte_collection: Cell<u128>,
    native_operations: LeaseTable,
    storage_borrows: Cell<usize>,
    native_scan_workspace: RefCell<ScanWorkspace>,
    pub(crate) graph: GraphState,
    graph_scan: RefCell<Option<ActiveScan>>,
    collection: RefCell<Option<Box<CollectionCycle>>>,
    pub(crate) native_applications:
        RefCell<SharedScanStore<NativeApplication, NativeApplicationScanId>>,
    pub(crate) interface_calls: RefCell<SharedScanStore<ScopedInterfaceCall, InterfaceCallScanId>>,
    pub(crate) interface_descriptions:
        RefCell<SharedScanStore<InterfaceDescription, InterfaceDescriptionScanId>>,
    // Heap-local immutable codecs contain no object identity or loaded types.
    // Initialize only when used; descriptor construction cannot execute callbacks.
    array_storage: OnceCell<NativeStorage>,
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

    pub(crate) fn ensure_no_storage_borrow(&self) -> Result<(), RuntimeError> {
        if self.storage_borrows.get() != 0 {
            return Err(RuntimeError::module_validation(
                "script reentry during heap storage access",
            ));
        }
        Ok(())
    }

    pub(crate) fn ensure_execution_allowed(&self) -> Result<(), RuntimeError> {
        self.ensure_no_storage_borrow()?;
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
            native_operations: Default::default(),
            storage_borrows: Cell::new(0),
            native_scan_workspace: RefCell::default(),
            graph: GraphState::default(),
            graph_scan: RefCell::default(),
            collection: RefCell::default(),
            native_applications: RefCell::new(SharedScanStore::new(owner)),
            interface_calls: RefCell::new(SharedScanStore::new(owner)),
            interface_descriptions: RefCell::new(SharedScanStore::new(owner)),
            array_storage: OnceCell::new(),
            cursor_storage: OnceCell::new(),
            objects: RefCell::new(ObjectStorage::default()),
            struct_layouts: RefCell::new(StructStorageLayouts::default()),

            roots: RefCell::new(RootTable::new(owner)),
            environments: RefCell::new(EnvironmentStore::new(owner)),
            interfaces: RefCell::new(InterfaceStore::new(owner)),
            applications: RefCell::new(ApplicationStore::new(owner)),
            operation_groups: RefCell::new(OperationGroupStore::new(owner)),
            stats: RefCell::new(CollectorStats::default()),
            resources,
            next_collection: Cell::new(config.collection_threshold.unwrap_or(usize::MAX).max(1)),
            next_byte_collection: Cell::new(config.retained_byte_threshold.max(1) as u128),
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
            current_retained_bytes: counters.current_retained_bytes,
            peak_retained_bytes: counters.peak_retained_bytes,
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
    ) -> Result<GcRef, RuntimeError> {
        self.ensure_execution_allowed()?;
        let storage = self.prepare_struct_storage(layout, fields)?;
        self.alloc_object(HeapObject::Struct { storage })
    }

    pub(crate) fn alloc_enum(
        &self,
        tag: EnumTag,
        fields: Vec<Value>,
    ) -> Result<GcRef, RuntimeError> {
        let failure = matches!(&tag, EnumTag::Declared(layout) if layout.variant().reports_failure);
        self.alloc_enum_origin(tag, fields, || {
            failure.then(|| ErrorTrace::capture(&self.resources))
        })
    }

    /// The caller validates the source variant before forwarding its optional origin.
    pub(crate) fn alloc_enum_origin(
        &self,
        tag: EnumTag,
        fields: Vec<Value>,
        origin: impl FnOnce() -> Option<Arc<ErrorTrace>>,
    ) -> Result<GcRef, RuntimeError> {
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
        let trace = origin();
        self.alloc_object(HeapObject::Enum(EnumValueSnapshot { tag, fields }, trace))
    }

    /// Diagnostic-only metadata; never participates in equality or key hashing.
    /// Query checked heap metadata for protected backend transport.
    ///
    /// # Safety
    /// Keep managed inputs live through this query. Type/owner validation cannot
    /// establish the lifetime of an expired direct reference.
    /// Direct references require live retention even for rejection probes. Use a
    /// retained wrong-kind/foreign value or a checked root token to test rejection;
    /// never inspect a reclaimed direct address.
    #[doc(hidden)]
    pub unsafe fn result_error_trace_transport(&self, value: &Value) -> Option<Arc<ErrorTrace>> {
        self.result_error_trace(value)
    }

    pub(crate) fn result_error_trace(&self, value: &Value) -> Option<Arc<ErrorTrace>> {
        let Value::Enum(id) = value else {
            return None;
        };
        let objects = self.objects.borrow();
        match self.identity_ref(&objects, *id)? {
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
    ) -> Result<GcRef, RuntimeError> {
        self.ensure_execution_allowed()?;
        let (Value::Enum(original), Value::Enum(value)) = (original, value) else {
            return Err(RuntimeError::module_validation("enum origin carriers"));
        };
        let (snapshot, trace) = {
            let objects = self.objects.borrow();
            let Some(HeapObject::Enum(_, trace)) = self.identity_ref(&objects, *original) else {
                return Err(RuntimeError::module_validation("origin carrier handle"));
            };
            let Some(HeapObject::Enum(snapshot, _)) = self.identity_ref(&objects, *value) else {
                return Err(RuntimeError::module_validation("enum value handle"));
            };
            (snapshot.clone(), trace.clone())
        };
        self.alloc_object(HeapObject::Enum(snapshot, trace))
    }

    pub(crate) fn ensure_structure_mutable(&self, id: GcRef) -> Result<(), RuntimeError> {
        self.check_native_operation(id, None, true)?;
        Ok(())
    }

    pub(crate) fn enum_layout(&self, id: GcRef) -> Option<EnumVariantRef> {
        self.with_enum(id, |snapshot| {
            let EnumTag::Declared(layout) = &snapshot.tag;
            layout.clone()
        })
    }

    /// Copy tag and fields for an owning inspection boundary. This copy does not
    /// root payload handles; retain their source root across allocation or reentry.
    /// Synchronous reads should use `enum_view` and release it before either boundary.
    /// Read a checked backend snapshot without adding independent retention.
    ///
    /// # Safety
    /// Keep source objects live through the read. Protect copied managed edges
    /// or identities before callbacks, collection or source mutation can remove
    /// their last traced owner. A returned snapshot is not a root.
    /// Direct references require live retention even for rejection probes. Use a
    /// retained wrong-kind/foreign value or a checked root token to test rejection;
    /// never inspect a reclaimed direct address.
    #[doc(hidden)]
    pub unsafe fn enum_snapshot_transport(&self, id: GcRef) -> Option<EnumValueSnapshot> {
        self.enum_snapshot(id)
    }

    pub(crate) fn enum_snapshot(&self, id: GcRef) -> Option<EnumValueSnapshot> {
        self.enum_view(id).map(|view| (*view).clone())
    }

    /// Query checked heap metadata for protected backend transport.
    ///
    /// # Safety
    /// Keep managed inputs live through this query. Type/owner validation cannot
    /// establish the lifetime of an expired direct reference.
    /// Retain the original root/traced owner; never query an allocation after
    /// reclamation. Host root tokens retain their separate generation checks.
    #[doc(hidden)]
    pub unsafe fn object_kind_transport(&self, id: GcRef) -> Option<GcObjectKind> {
        self.identity_kind(id)
    }

    pub(crate) fn identity_kind(&self, id: GcRef) -> Option<GcObjectKind> {
        let objects = self.objects.try_borrow().ok()?;
        Self::object_kind_at(&objects, objects.direct_location(id)?)
    }

    fn object_kind_at(objects: &ObjectStorage, location: PayloadLocation) -> Option<GcObjectKind> {
        Some(match location.kind {
            PayloadKind::String => GcObjectKind::String,
            PayloadKind::Tuple => GcObjectKind::Tuple,
            PayloadKind::Range => GcObjectKind::Range,
            PayloadKind::HostRoot => GcObjectKind::HostRoot,
            PayloadKind::HostPath => GcObjectKind::HostPath,
            PayloadKind::Ephemeral => GcObjectKind::Ephemeral,
            PayloadKind::Native => match objects.native(location)?.ty() {
                Ty::Array(..) => GcObjectKind::Array,

                Ty::Iter(..) => GcObjectKind::Iter,
                _ => GcObjectKind::Native,
            },
            PayloadKind::Enum => GcObjectKind::Enum,
            PayloadKind::Struct => GcObjectKind::Struct,
            PayloadKind::Interface => GcObjectKind::Interface,
            PayloadKind::Closure => GcObjectKind::Closure,
            PayloadKind::Cell => GcObjectKind::Cell,
            PayloadKind::Taken => return None,
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
        snapshot
            .directory_index()
            .ok_or_else(|| self.resource_limit("interface snapshot locator"))?;
        drop(view);
        self.publish_heap_identity(GraphIdentity::Interface(snapshot))?;
        self.alloc_object(HeapObject::Interface { snapshot })
            .map(InterfaceObjectId)
    }

    pub(crate) fn alloc_closure(
        &self,
        modules: &ModuleStore,
        snapshot: ClosureValueSnapshot,
    ) -> Result<GcRef, RuntimeError> {
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
        self.publish_metadata_edge(modules, MetadataEdge::Closure(&snapshot))?;
        self.alloc_object(HeapObject::Closure { snapshot })
    }

    pub(crate) fn alloc_cell(&self, ty: ValueType, value: Value) -> Result<GcRef, RuntimeError> {
        self.ensure_execution_allowed()?;
        if !self.valid_payload(&value) || !value.has_representation(ty) {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "invalid cell value",
            ));
        }
        self.alloc_object(HeapObject::Cell { ty, value })
    }

    pub(crate) fn cell_get(&self, id: GcRef, ty: ValueType) -> Result<Value, RuntimeError> {
        let objects = self.objects.borrow();
        match self.readable_identity(&objects, id) {
            Some(HeapObject::Cell { ty: actual, value }) if *actual == ty => Ok(*value),
            _ => Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "invalid capture cell",
            )),
        }
    }

    pub(crate) fn captured_cell_value(&self, id: GcRef) -> Option<Value> {
        let objects = self.objects.borrow();
        match self.readable_identity(&objects, id)? {
            HeapObject::Cell { ty, value } if value.has_representation(*ty) => Some(*value),
            _ => None,
        }
    }

    pub(crate) fn cell_set(
        &self,
        id: GcRef,
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
        self.publish_value(value)?;
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

    pub(crate) fn closure_snapshot(&self, id: GcRef) -> Option<Ref<'_, ClosureValueSnapshot>> {
        Ref::filter_map(self.objects.try_borrow().ok()?, |objects| {
            match self.readable_identity(objects, id)? {
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
        match self.readable_identity(&objects, id.0)? {
            HeapObject::Interface { snapshot, .. } => Some(snapshot),
            _ => None,
        }
    }

    pub(crate) fn interface_snapshot(
        &self,
        id: InterfaceObjectId,
    ) -> Option<Ref<'_, InterfaceValueSnapshot>> {
        self.interface_metadata(self.interface_snapshot_id(id)?)
    }

    /// Read a checked backend snapshot without adding independent retention.
    ///
    /// # Safety
    /// Keep source objects live through the read. Protect copied managed edges
    /// or identities before callbacks, collection or source mutation can remove
    /// their last traced owner. A returned snapshot is not a root.
    /// Retain the original root/traced owner; never query an allocation after
    /// reclamation. Host root tokens retain their separate generation checks.
    #[doc(hidden)]
    pub unsafe fn trace_roots_transport(&self) -> Option<Vec<GcRef>> {
        self.trace_roots()
    }

    pub(crate) fn trace_roots(&self) -> Option<Vec<GcRef>> {
        self.trace_values(&self.root_snapshots()?)
    }

    pub(crate) fn trace_value(&self, value: &Value) -> Option<Vec<GcRef>> {
        self.trace_values(slice::from_ref(value))
    }

    pub(crate) fn automatic_collection_enabled(&self) -> bool {
        self.config.collection_threshold.is_some()
    }

    pub(crate) fn collection_due(&self) -> bool {
        self.config.collection_threshold.is_some()
            && (self
                .resources
                .counters()
                .current_heap_units
                .saturating_add(self.operation_groups.borrow().count())
                .saturating_add(self.applications.borrow().count())
                .saturating_add(self.interfaces.borrow().count())
                .saturating_add(self.environments.borrow().count())
                >= self.next_collection.get()
                || self.resources.counters().current_retained_bytes
                    >= self.next_byte_collection.get())
    }

    /// Query checked heap metadata for protected backend transport.
    ///
    /// # Safety
    /// Keep managed inputs live through this query. Type/owner validation cannot
    /// establish the lifetime of an expired direct reference.
    /// Direct references require live retention even for rejection probes. Use a
    /// retained wrong-kind/foreign value or a checked root token to test rejection;
    /// never inspect a reclaimed direct address.
    #[doc(hidden)]
    pub unsafe fn validate_value_transport(&self, value: &Value) -> bool {
        self.validate_value(value)
    }

    pub(crate) fn validate_value(&self, value: &Value) -> bool {
        if !value.is_managed() {
            return true;
        }
        self.objects
            .try_borrow()
            .is_ok_and(|objects| objects.validate_value(value).is_some())
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
            let Some(object) = self.identity_ref(&objects, id) else {
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

    fn alloc_object(&self, object: HeapObject) -> Result<GcRef, RuntimeError> {
        // Publish candidate fields before transferring storage. Native admission
        // already visits its edges through the provider's scoped scanner.
        match &object {
            HeapObject::Taken => unreachable!("tombstones only replace admitted Native owners"),
            HeapObject::Tuple(tuple) => {
                for value in &tuple.members {
                    self.publish_value(*value)?;
                }
            }
            HeapObject::Enum(snapshot, _) => {
                for value in &snapshot.fields {
                    self.publish_value(*value)?;
                }
            }
            HeapObject::Struct { storage } => {
                for value in &storage.values {
                    self.publish_value(*value)?;
                }
            }
            HeapObject::Closure { snapshot } => {
                for value in &snapshot.captures {
                    self.publish_value(*value)?;
                }
                if let Some(environment) = &snapshot.environment {
                    self.publish_heap_identity(GraphIdentity::Environment(environment.id))?;
                }
            }
            HeapObject::Cell { value, .. } => self.publish_value(*value)?,
            HeapObject::HostPath(path) => {
                for value in path.retained_values() {
                    self.publish_value(*value)?;
                }
            }
            HeapObject::Interface { snapshot } => {
                self.publish_heap_identity(GraphIdentity::Interface(*snapshot))?
            }
            HeapObject::String(_)
            | HeapObject::Range(_)
            | HeapObject::HostRoot(_)
            | HeapObject::Ephemeral(_)
            | HeapObject::Native(_) => {}
        }
        let charge = object.charge(&self.interfaces).ok_or_else(|| {
            RuntimeError::module_validation("invalid or borrowed interface accounting")
        })?;
        let growth = self
            .resources
            .prepare_heap_change(HeapCharge::default(), charge.logical_units)?;
        let initialization_owner = self
            .resources
            .active_session()
            .filter(|session| session.options.phase == ExecutionPhase::CandidateInitialization)
            .map(|session| session.root.program_root().key());
        let mut objects = self.objects_mut()?;
        if initialization_owner.is_some() {
            objects
                .initialization_owners
                .try_reserve(1)
                .map_err(|_| self.resource_limit("initialization owners"))?;
        }
        // Shared slab/large backing is charged only on acquisition, independently
        // of per-object indirect storage. Preflight before transferring ownership.
        charge
            .retained_bytes
            .checked_add(ObjectStorage::backing_bound(&object)?)
            .ok_or_else(|| self.resource_limit("payload backing charge"))?;
        let mut graph = self
            .graph
            .try_lock()
            .map_err(|_| RuntimeError::module_validation("graph admission is borrowed"))?;
        graph.prepare_identity()?;
        let before_backing = objects.backing_bytes();
        let location = objects.insert(object)?;
        let charge = charge
            .with_shallow_bytes(objects.backing_bytes() - before_backing)
            .expect("preflighted payload backing charge");
        objects.publish_direct();
        let id = objects
            .direct_reference(location)
            .expect("initialized managed header");
        graph.admit_prepared(id.graph(), objects.mark(location).expect("admitted mark"));
        if let Some(owner) = initialization_owner {
            objects.initialization_owners.insert(id, owner);
        }
        self.stats.borrow_mut().allocated_objects += 1;
        growth.commit(charge);
        Ok(id)
    }

    fn object_mut<'a>(
        &self,
        objects: &'a mut ObjectStorage,
        id: GcRef,
    ) -> Option<HeapObject<Exclusive<'a>>> {
        let location = self.readable_identity_location(objects, id)?;
        objects.get_mut(location, &self.interfaces)
    }

    fn native_ref<'a>(&self, objects: &'a ObjectStorage, id: GcRef) -> Option<&'a NativeObject> {
        self.readable_direct(objects, id)?.native()
    }

    fn native_mut<'a>(
        &self,
        objects: &'a mut ObjectStorage,
        id: GcRef,
    ) -> Option<&'a mut NativeObject> {
        objects.native_mut(id, |objects, resolved| {
            self.admit_identity(objects, id, || resolved.mark)
        })
    }

    fn value_identity(&self, objects: &ObjectStorage, value: &Value) -> Option<GcRef> {
        let identity = value.direct_reference()?;
        objects.direct_location(identity)?;
        Some(identity)
    }

    fn readable_identity_location(
        &self,
        objects: &ObjectStorage,
        identity: GcRef,
    ) -> Option<PayloadLocation> {
        let location = objects.direct_location(identity)?;
        self.admit_identity(objects, identity, || {
            objects.mark(location).expect("occupied direct mark")
        })?;
        Some(location)
    }

    /// Read admission retains the physical evidence until its typed view ends.
    /// Native mutable access scopes resolution/admission inside its exclusive
    /// directory borrow; all paths share graph and initialization policy.
    fn readable_direct<'a>(
        &self,
        objects: &'a ObjectStorage,
        identity: GcRef,
    ) -> Option<ResolvedDirect<'a>> {
        let resolved = objects.resolve_direct(identity)?;
        self.admit_identity(objects, identity, || resolved.mark)?;
        Some(resolved)
    }

    fn admit_identity<'a>(
        &self,
        objects: &ObjectStorage,
        identity: GcRef,
        mark: impl FnOnce() -> MarkRef<'a>,
    ) -> Option<()> {
        if !self.graph.with_access(mark).ok()? {
            return None;
        }
        if let Some(session) = self
            .resources
            .active_session()
            .filter(|session| session.options.phase == ExecutionPhase::CandidateInitialization)
            && objects.initialization_owners.get(&identity)
                != Some(&session.root.program_root().key())
        {
            return None;
        }
        Some(())
    }

    fn identity_ref<'a>(
        &self,
        objects: &'a ObjectStorage,
        identity: GcRef,
    ) -> Option<HeapObject<Shared<'a>>> {
        objects.resolve_direct(identity)?.object(&self.interfaces)
    }

    fn readable_identity<'a>(
        &self,
        objects: &'a ObjectStorage,
        identity: GcRef,
    ) -> Option<HeapObject<Shared<'a>>> {
        self.readable_direct(objects, identity)?
            .object(&self.interfaces)
    }

    fn with_struct<R>(&self, reference: GcRef, f: impl FnOnce(StructView<'_>) -> R) -> Option<R> {
        let objects = self.objects.borrow();
        Some(f(self.readable_direct(&objects, reference)?.fields()?))
    }

    fn with_struct_mut<R>(
        &self,
        reference: GcRef,
        f: impl FnOnce(StructViewMut<'_>) -> R,
    ) -> Option<R> {
        let mut objects = self.objects_mut().ok()?;
        let location = self.readable_identity_location(&objects, reference)?;
        Some(f(objects.structure_mut(location)?))
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod array_bulk_tests;
