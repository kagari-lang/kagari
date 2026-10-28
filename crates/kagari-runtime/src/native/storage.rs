//! One traced Rust payload mechanism for native script-heap objects.
//! Factories and Rust type identity are local installation data, never portable ABI.
pub mod access;
mod erasure;
pub mod scan;
use crate::{
    Runtime,
    error::RuntimeError,
    execution_metadata::MetadataEdge,
    frame::types::{
        arguments::{TypeArgument, type_parameter},
        compatibility::TypeView,
    },
    gc::{GcHeap, roots::RootedValue},
    module::ExecutableModule,
    native::{
        binding::NativeResult,
        context::{LinkedCallable, LinkedOperation},
        payload::{data::NativeData, managed::ManagedSchema, take::NativeTake},
        storage::{
            erasure::{Erasure, Payload},
            scan::{NativeScanKind, ScanBudget, ScanEntries, ScanStatus, ScanWorkspace},
        },
        storage_type::StorageType,
        stored_selection::StoredSelection,
    },
    resource::HeapCharge,
    value::Value,
};
use kagari_common::identity::table::DefinitionId;
use kagari_types::{declaration::native::NativeStorageLayout, ty::Ty};
use std::{
    alloc::Layout,
    any::{Any, TypeId},
    fmt,
    fmt::Debug,
    mem,
    ptr::NonNull,
    slice,
    sync::Arc,
};

/// Runtime-owned Rust storage with a single scanner and committed backing charge.
///
/// # Safety
/// Report every strong internal script edge, including temporary algorithm storage.
/// Edge, charge and structural mutations must use checked publication/accounting
/// services, never interior mutability behind a shared loan. Before any callback, release all payload loans
/// and protect extracted values with roots or other traced edges. Every installed
/// state, including errors and unwinds, must remain scan-valid and charge-valid.
/// Scanning/charging cannot allocate, reenter, mutate the payload or panic. Cursor
/// state must contain no payload pointers, roots, references or untraced graph edges.
/// Providers must honor their declared work guarantee and make finite progress.
pub unsafe trait NativePayload: Any + Debug + Send {
    type ScanState: Copy + Send + 'static;

    const SCAN_KIND: NativeScanKind;

    fn begin_scan(&self) -> Self::ScanState;

    fn scan<'payload>(
        &'payload self,
        state: &mut Self::ScanState,
        budget: &mut ScanBudget,
        visit: &mut dyn FnMut(&'payload Value),
    ) -> ScanStatus;

    /// Describe the current payload in constant time, including during unwind. Logical units exclude the heap
    /// identity; bytes include exclusively owned indirect backing, not this Rust
    /// value's shallow size (the runtime adds that exactly once), referenced GC
    /// objects, shared metadata or external host resources. State whether indirect
    /// bytes are known capacity or an estimate. Document unobservable allocations
    /// as excluded rather than claiming an exact zero; external host resources stay
    /// outside this charge. This is GC pressure, not a quota or allocator/RSS total.
    ///
    /// This callback must be side-effect-free and cannot reenter the runtime.
    /// Admission and every completed general edit sample this infallible callback;
    /// release uses the committed record and never calls it. Installed charge
    /// must remain stable except through checked runtime storage mutations; a
    /// provider must not resize attributed backing through interior mutability.
    fn charge(&self) -> HeapCharge;

    /// Local storage policy for automatic Opaque schema registration. The default
    /// grants unsafe provider access, not safe data loans. Take/data policies carry their
    /// existing capability proof; the registration owner checks the Rust identity.
    fn registration_storage() -> NativeStorage
    where
        Self: Sized,
    {
        NativeStorage::payload::<Self>()
    }
}

pub struct StorageContext<'call> {
    pub(crate) runtime: &'call Runtime,
    pub(crate) owner: &'call ExecutableModule,
    pub(crate) ty: &'call Ty,
    pub(crate) scope: Option<&'call TypeArgument>,
    pub(crate) selected: &'call [LinkedOperation],
}

impl<'call> StorageContext<'call> {
    pub fn heap(&self) -> &'call GcHeap {
        self.runtime.gc()
    }

    /// Prepare a concrete type whose IDs belong to this call's loaded generation.
    pub fn resolve_type(&self, ty: &Ty) -> NativeResult<TypeArgument> {
        self.runtime
            .resolve_type_arguments(self.owner, slice::from_ref(ty))?
            .pop()
            .ok_or_else(|| RuntimeError::module_validation("native storage type scope"))
    }

    pub fn allocate_array(
        &self,
        element: TypeArgument,
        elements: Vec<Value>,
    ) -> NativeResult<Value> {
        self.heap().ensure_no_native_borrow()?;
        element.validate(self.runtime)?;
        self.runtime.validate_heap_payloads(&elements)?;
        let contract = Arc::new(StorageType::prepare_scoped(element, self.owner)?);
        self.heap()
            .alloc_array_with_contract(contract, elements)
            .map(Value::Array)
    }

    pub fn type_parameter(&self, index: usize) -> NativeResult<TypeArgument> {
        match self.scope {
            Some(scope) => scope.parameter(self.runtime, self.owner, index),
            None => self
                .runtime
                .resolve_type_arguments(
                    self.owner,
                    slice::from_ref(type_parameter(self.ty, index).ok_or_else(|| {
                        RuntimeError::module_validation("native storage type parameter")
                    })?),
                )?
                .pop()
                .ok_or_else(|| RuntimeError::module_validation("native storage type scope")),
        }
    }

    pub fn element_contract(&self, index: usize) -> NativeResult<Arc<StorageType>> {
        StorageType::prepare_scoped(self.type_parameter(index)?, self.owner).map(Arc::new)
    }

    pub fn owner(&self) -> &'call ExecutableModule {
        self.owner
    }

    pub fn selected(&self, slot: usize) -> NativeResult<&'call LinkedCallable> {
        self.selected
            .get(slot)
            .and_then(LinkedOperation::ready)
            .ok_or_else(|| {
                RuntimeError::module_validation("native storage requires a prepared callable slot")
            })
    }

    pub fn ty(&self) -> &'call Ty {
        self.ty
    }
}

type Factory =
    dyn for<'call> Fn(&StorageContext<'call>) -> NativeResult<Box<dyn Any + Send>> + Send + Sync;

type Charge = dyn Fn(&dyn Any) -> HeapCharge + Send + Sync;

struct StorageEntries {
    rust_type: TypeId,
    erasure: Erasure,
    layout: NativeStorageLayout,
    factory: Option<Box<Factory>>,
    scan: ScanEntries,
    charge: Box<Charge>,
    editable_data: bool,
    takeable: bool,
    managed: Option<Arc<ManagedSchema>>,
}

/// An immutable checked erasure of one Rust payload type. Clone shares entries.
#[derive(Clone)]
pub struct NativeStorage {
    entries: Arc<StorageEntries>,
}

impl Debug for NativeStorage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NativeStorage")
            .field("layout", &self.entries.layout)
            .field("rust_type", &self.entries.rust_type)
            .finish_non_exhaustive()
    }
}

impl NativeStorage {
    pub(crate) fn with_managed_schema(mut self, schema: ManagedSchema) -> Self {
        Arc::get_mut(&mut self.entries)
            .expect("new storage descriptor")
            .managed = Some(Arc::new(schema));
        self
    }

    pub(crate) fn managed_schema(&self) -> Option<&Arc<ManagedSchema>> {
        self.entries.managed.as_ref()
    }

    /// Safe direct editing is available only for recursively checked fixed data.
    /// Handwritten traced payload registrations do not receive this capability.
    pub fn data<S: NativeData>() -> Self {
        let mut storage = Self::takeable::<S>();
        Arc::get_mut(&mut storage.entries)
            .expect("new storage descriptor")
            .editable_data = true;
        storage
    }

    pub(crate) fn is_editable_data(&self) -> bool {
        self.entries.editable_data
    }

    pub(crate) fn accepts_payload<S: 'static>(&self) -> bool {
        self.entries.rust_type == TypeId::of::<S>()
    }

    pub fn new<S: NativePayload>(
        factory: impl for<'call> Fn(&StorageContext<'call>) -> NativeResult<S> + Send + Sync + 'static,
    ) -> Self {
        Self::with_layout(NativeStorageLayout::Opaque, factory)
    }

    pub(crate) fn with_layout<S: NativePayload>(
        layout: NativeStorageLayout,
        factory: impl for<'call> Fn(&StorageContext<'call>) -> NativeResult<S> + Send + Sync + 'static,
    ) -> Self {
        Self::entries::<S>(
            layout,
            Some(Box::new(move |context| {
                factory(context).map(|payload| Box::new(payload) as Box<dyn Any + Send>)
            })),
        )
    }

    /// Register a payload that native constructors explicitly provide. There is
    /// no default factory for objects whose state depends on constructor inputs.
    pub fn payload<S: NativePayload>() -> Self {
        Self::provided_with_layout::<S>(NativeStorageLayout::Opaque)
    }

    /// Register edge-free Rust-owned payloads with explicit extraction capability.
    /// Traced payloads continue using `payload`; no empty-trace inference occurs.
    pub fn takeable<S: NativeTake>() -> Self {
        let mut storage = Self::payload::<S>();
        Arc::get_mut(&mut storage.entries)
            .expect("new storage descriptor")
            .takeable = true;
        storage
    }

    pub(crate) fn is_takeable(&self) -> bool {
        self.entries.takeable
    }

    pub(crate) fn provided_with_layout<S: NativePayload>(layout: NativeStorageLayout) -> Self {
        Self::entries::<S>(layout, None)
    }

    fn entries<S: NativePayload>(
        layout: NativeStorageLayout,
        factory: Option<Box<Factory>>,
    ) -> Self {
        Self {
            entries: Arc::new(StorageEntries {
                rust_type: TypeId::of::<S>(),
                erasure: Erasure::new::<S>(),
                layout,
                factory,
                scan: ScanEntries::new::<S>(),
                charge: Box::new(|payload| {
                    payload
                        .downcast_ref::<S>()
                        .expect("factory and size entries share the sealed Rust payload type")
                        .charge()
                }),
                editable_data: false,
                takeable: false,
                managed: None,
            }),
        }
    }

    pub(crate) fn installed_charge(&self, payload: &(dyn Any + Send)) -> HeapCharge {
        (self.entries.charge)(payload)
    }

    pub(crate) fn scan_layout(&self) -> Layout {
        self.entries.scan.layout
    }

    pub(crate) fn scan_payload<'a>(
        &self,
        payload: &'a dyn Any,
        visit: &mut dyn FnMut(&'a Value),
    ) -> NativeResult<()> {
        self.entries.scan.inspect(payload, visit)
    }

    pub fn layout(&self) -> NativeStorageLayout {
        self.entries.layout
    }

    pub(crate) fn create(&self, context: &StorageContext<'_>) -> NativeResult<NativeObject> {
        let factory = self.entries.factory.as_ref().ok_or_else(|| {
            RuntimeError::module_validation(
                "native storage requires an explicitly supplied payload",
            )
        })?;
        let payload = factory(context)?;
        let mut object = self.object(context.heap(), context.ty, payload, context.owner)?;
        object.scope = context.scope.cloned();
        Ok(object)
    }

    /// One admission/publication path for typed opaque and checked low-level native results.
    pub(crate) fn allocate_payload<S: NativePayload>(
        &self,
        context: &StorageContext<'_>,
        payload: S,
        selected: Arc<[StoredSelection]>,
    ) -> NativeResult<RootedValue> {
        let runtime = context.runtime;
        runtime.gc().ensure_no_native_borrow()?;
        runtime.resources().ensure_execution_allowed()?;
        runtime.validate_executable_module(context.owner)?;
        let scope = context.scope.ok_or_else(|| {
            RuntimeError::module_validation("native allocation requires a closed result scope")
        })?;
        scope.validate(runtime)?;
        runtime.resources().poll_execution()?;
        let mut object = self.prepare_payload(runtime.gc(), context.ty, payload, context.owner)?;
        object.scope = Some(scope.clone());
        runtime.publish_metadata_edge(MetadataEdge::Selections(&selected))?;
        object.selected = selected;
        let id = runtime.gc().alloc_native(object)?;
        let root = runtime
            .root_value(Value::GcHandle(id))
            .ok_or_else(|| RuntimeError::module_validation("native object retention"))?;
        runtime.gc_safepoint()?;
        Ok(root)
    }

    pub(crate) fn prepare_payload<S: NativePayload>(
        &self,
        heap: &GcHeap,
        ty: &Ty,
        payload: S,
        owner: &ExecutableModule,
    ) -> NativeResult<NativeObject> {
        if self.entries.rust_type != TypeId::of::<S>() {
            return Err(RuntimeError::module_validation(
                "native allocation payload type mismatch",
            ));
        }
        self.object(heap, ty, Box::new(payload), owner)
    }

    fn object(
        &self,
        heap: &GcHeap,
        ty: &Ty,
        payload: Box<dyn Any + Send>,
        owner: &ExecutableModule,
    ) -> NativeResult<NativeObject> {
        let mut valid = true;
        heap.scan_native_payload(self, payload.as_ref(), &mut |value| {
            valid &= value.is_default_heap_payload(heap) && heap.publish_value(*value).is_ok();
        })?;
        if !valid {
            return Err(RuntimeError::module_validation(
                "native factory retained an invalid script value",
            ));
        }
        let charge = (self.entries.charge)(payload.as_ref());
        Ok(NativeObject {
            charge,
            storage: self.clone(),
            payload: Payload::Staged(payload),
            ty: ty.clone(),
            _owner: owner.clone(),
            scope: None,
            selected: Arc::from([]),
            revision: 0,
        })
    }
}

/// GC owns the payload and immutable type metadata. Only actual executable
/// dependencies (stored callbacks or selected key operations) retain module instances;
/// data alone must not
/// create a module-state -> data -> module-instance retention cycle.
pub(crate) struct NativeObject {
    pub(crate) storage: NativeStorage,
    /// Runtime-owned mutation state travels with this identity's native record.
    /// Replacing its Rust payload preserves it; a new allocation starts at zero.
    pub(crate) revision: u64,
    /// Committed indirect payload charge; slab/large backing is accounted separately.
    pub(crate) charge: HeapCharge,
    payload: Payload,
    pub(crate) ty: Ty,
    _owner: ExecutableModule,
    pub(crate) scope: Option<TypeArgument>,
    pub(crate) selected: Arc<[StoredSelection]>,
}

impl Debug for NativeObject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NativeObject")
            .field("ty", &self.ty)
            .field("storage", &self.storage)
            .finish_non_exhaustive()
    }
}

impl NativeObject {
    #[cfg(feature = "execution-diagnostics")]
    pub(crate) fn payload_bytes(&self) -> usize {
        self.storage.entries.erasure.layout.size()
    }

    pub(crate) fn physical_layout(&self) -> NativeResult<(Layout, usize)> {
        Layout::new::<Self>()
            .extend(self.storage.entries.erasure.layout)
            .map(|(layout, offset)| (layout.pad_to_align(), offset))
            .map_err(|_| RuntimeError::resource_limit("native inline layout"))
    }

    pub(crate) fn payload_type(&self) -> TypeId {
        self.storage.entries.rust_type
    }

    /// # Safety
    /// Destination is disjoint, aligned, uninitialized storage for this descriptor's T,
    /// retained alongside this header until its destruction or checked take.
    pub(crate) unsafe fn place_payload(mut self, destination: NonNull<u8>) -> Self {
        let Payload::Staged(value) = mem::replace(&mut self.payload, Payload::Taken) else {
            unreachable!("only unpublished native objects are placed")
        };
        // SAFETY: caller reserves the checked combined layout before placement.
        unsafe { (self.storage.entries.erasure.place)(value, destination) };
        self.payload = Payload::Inline {
            pointer: destination,
            loaned: false,
        };
        self
    }

    pub(crate) fn take_payload<S: NativeTake>(&mut self) -> S {
        assert!(
            self.storage.accepts_payload::<S>(),
            "checked native take TypeId"
        );
        let Payload::Inline {
            pointer,
            loaned: false,
        } = mem::replace(&mut self.payload, Payload::Taken)
        else {
            unreachable!("checked live inline extraction")
        };
        // SAFETY: checked TypeId, exclusive retired allocation and initialized T.
        // Taken prevents Drop from touching the moved value.
        unsafe { pointer.cast::<S>().as_ptr().read() }
    }

    pub(crate) fn owner(&self) -> &ExecutableModule {
        &self._owner
    }

    // Linking compares every native storage contract with its installed owner.
    // Installation forbids replacing that owner while its heap objects exist.
    pub(crate) fn definition_name(&self, id: DefinitionId) -> Option<&str> {
        self._owner.definition_name(id)
    }

    pub(crate) fn matches_argument(
        &self,
        expected: &TypeArgument,
        owner: &ExecutableModule,
    ) -> bool {
        // These immutable identities include every nested script nominal's exact
        // runtime, installation and publication. Reuse the existing type-argument
        // owner rather than reconstructing a layout graph on every native call.
        // Unequal identities may still describe reload-compatible layouts, so
        // they retain the complete existing compatibility check below.
        if let Some(actual) = &self.scope
            && let (Some(actual), Some(expected)) = (
                actual.borrowed_identity(&self._owner),
                expected.borrowed_identity(owner),
            )
            && actual == expected
        {
            return true;
        }
        self.matches(expected.view(owner))
    }

    pub(crate) fn matches(&self, expected: TypeView<'_>) -> bool {
        let actual = match &self.scope {
            Some(scope) => scope.view(&self._owner),
            None => TypeView::new(&self.ty, &self._owner, None),
        };
        // Preserve the caller's closed proof and lexical scope together. Rebuilding
        // a view from its type spelling would repeat preparation for every operand.
        actual.compatible(expected)
    }

    pub(crate) fn begin_scan(&self, workspace: &mut ScanWorkspace) -> NativeResult<()> {
        let payload = self.payload.view(&self.storage.entries.erasure)?;
        self.storage.entries.scan.begin(payload, workspace)
    }

    pub(crate) fn scan_slice<'payload>(
        &'payload self,
        heap: &GcHeap,
        workspace: &mut ScanWorkspace,
        budget: &mut ScanBudget,
        visit: &mut dyn FnMut(&'payload Value, &mut ScanBudget) -> NativeResult<ScanStatus>,
    ) -> NativeResult<ScanStatus> {
        let payload = self.payload.view(&self.storage.entries.erasure)?;
        self.storage
            .entries
            .scan
            .resume_traced(heap, payload, workspace, budget, visit)
    }

    pub(crate) fn inspect<'payload>(
        &'payload self,
        visit: &mut dyn FnMut(&'payload Value),
    ) -> NativeResult<()> {
        let payload = self.payload.view(&self.storage.entries.erasure)?;
        self.storage.entries.scan.inspect(payload, visit)
    }

    pub(crate) fn loan_payload(&mut self) -> NativeResult<NonNull<dyn Any + Send>> {
        self.payload.loan(&self.storage.entries.erasure)
    }

    pub(crate) fn restore_payload(&mut self) {
        self.payload.end_loan();
    }

    pub(crate) fn payload<S: 'static>(&self) -> NativeResult<&S> {
        self.payload
            .view(&self.storage.entries.erasure)?
            .downcast_ref()
            .ok_or_else(|| {
                RuntimeError::module_validation(
                    "native payload access codec differs from the installed storage",
                )
            })
    }

    pub(crate) fn payload_mut<S: 'static>(&mut self) -> NativeResult<&mut S> {
        self.payload
            .view_mut(&self.storage.entries.erasure)?
            .downcast_mut()
            .ok_or_else(|| {
                RuntimeError::module_validation(
                    "native payload access codec differs from the installed storage",
                )
            })
    }
}
