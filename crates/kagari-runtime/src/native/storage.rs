//! One traced Rust payload mechanism for native script-heap objects.
//! Factories and Rust type identity are local installation data, never portable ABI.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::{
        arguments::{TypeArgument, type_parameter},
        compatibility::TypeView,
    },
    gc::GcHeap,
    module::ExecutableModule,
    native::{
        binding::NativeResult,
        context::{LinkedCallable, LinkedOperation},
        payload::{data::NativeData, managed::ManagedSchema, take::NativeTake},
        storage_type::StorageType,
        stored_selection::StoredSelection,
    },
    resource::HeapCharge,
    value::Value,
};
use kagari_common::identity::table::DefinitionId;
use kagari_contract::standard::RuntimePrimitive;
use kagari_types::{declaration::native::NativeStorageLayout, ty::Ty};
use std::{
    any::{Any, TypeId},
    fmt,
    fmt::Debug,
    mem::size_of_val,
    slice,
    sync::Arc,
};

/// Trace every script value retained by this payload. The visitor cannot execute
/// scripts or retain a reference past tracing. Rust Drop owns payload destruction.
/// The runtime exclusively owns the payload, so Send permits transfer without
/// requiring Sync. Factories shared by multiple installations require Send + Sync.
pub trait NativePayload: Any + Debug + Send {
    fn trace<'payload>(&'payload self, visit: &mut dyn FnMut(&'payload Value));

    /// Describe a new payload before publication. Logical units exclude the heap
    /// identity; bytes include exclusively owned indirect backing, not this Rust
    /// value's shallow size (the runtime adds that exactly once), referenced GC
    /// objects, shared metadata or external host resources. State whether indirect
    /// bytes are known capacity or an estimate. Document unobservable allocations
    /// as excluded rather than claiming an exact zero; external host resources stay
    /// outside this charge. This is GC pressure, not a quota or allocator/RSS total.
    ///
    /// This callback must be side-effect-free and cannot reenter the runtime.
    /// The description is sampled once; release never calls it. Installed charge
    /// must remain stable except through checked runtime storage mutations; a
    /// provider must not resize attributed backing through interior mutability.
    fn charge(&self) -> NativeResult<HeapCharge>;

    /// Local storage policy for automatic Opaque schema registration. The default
    /// grants traced read access only. Explicit take/data policies must carry their
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

    pub(crate) fn element_contract(&self, index: usize) -> NativeResult<Arc<StorageType>> {
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

type Trace = dyn for<'payload> Fn(&'payload dyn Any, &mut dyn FnMut(&'payload Value)) + Send + Sync;

type Charge = dyn Fn(&dyn Any) -> NativeResult<HeapCharge> + Send + Sync;

struct StorageEntries {
    rust_type: TypeId,
    layout: NativeStorageLayout,
    factory: Option<Box<Factory>>,
    trace: Box<Trace>,
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
                layout,
                factory,
                trace: Box::new(|payload, visit| {
                    payload
                        .downcast_ref::<S>()
                        .expect("factory and trace entries share the sealed Rust payload type")
                        .trace(visit)
                }),
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
        if matches!(
            self.layout(),
            NativeStorageLayout::Map { .. } | NativeStorageLayout::Set { .. }
        ) {
            let hash = context.selected(0)?;
            let equal = context.selected(1)?;
            if hash.primitive != Some(RuntimePrimitive::ValueHash)
                || equal.primitive != Some(RuntimePrimitive::ValueEq)
            {
                object.selected = Arc::from([
                    StoredSelection::new(context.owner, hash)?,
                    StoredSelection::new(context.owner, equal)?,
                ]);
            }
        }
        Ok(object)
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
        (self.entries.trace)(payload.as_ref(), &mut |value| {
            valid &= value.is_default_heap_payload(heap) && heap.validate_value(value);
        });
        if !valid {
            return Err(RuntimeError::module_validation(
                "native factory retained an invalid script value",
            ));
        }
        let charge = (self.entries.charge)(payload.as_ref())?
            .with_shallow_bytes(size_of_val(payload.as_ref()))?;
        Ok(NativeObject {
            charge,
            storage: self.clone(),
            payload,
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
    /// Authoritative committed payload charge, including the erased Box's shallow
    /// allocation but excluding the heap identity. Moves preserve this record.
    pub(crate) charge: HeapCharge,
    payload: Box<dyn Any + Send>,
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
    pub(crate) fn payload_bytes(&self) -> usize {
        size_of_val(self.payload.as_ref())
    }

    pub(crate) fn into_payload<S: NativeTake>(self) -> S {
        // The GC preflights Rust identity before any detachment. This immutable
        // erased record cannot change payload types between validation and move.
        *self
            .payload
            .downcast::<S>()
            .expect("validated extraction payload type")
    }

    pub(crate) fn owner(&self) -> &ExecutableModule {
        &self._owner
    }

    // Linking compares every native storage contract with its installed owner.
    // Installation forbids replacing that owner while its heap objects exist.
    pub(crate) fn definition_name(&self, id: DefinitionId) -> Option<&str> {
        self._owner.definition_name(id)
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

    pub(crate) fn trace<'payload>(&'payload self, visit: &mut dyn FnMut(&'payload Value)) {
        (self.storage.entries.trace)(self.payload.as_ref(), visit);
    }

    /// Sealed storage owners describe the completed edit without invoking an
    /// erased provider callback. All indirect allocations are disjoint from Box.
    pub(crate) fn storage_charge(&self, charge: HeapCharge) -> HeapCharge {
        charge
            .with_shallow_bytes(self.payload_bytes())
            .expect("owned storage allocations fit the address space")
    }

    pub(crate) fn replaced_payload<S: NativePayload>(&self, payload: S) -> NativeResult<Self> {
        if self.storage.entries.rust_type != TypeId::of::<S>() {
            return Err(RuntimeError::module_validation(
                "native replacement payload type mismatch",
            ));
        }
        let charge = payload
            .charge()?
            .with_shallow_bytes(size_of_val(&payload))?;
        Ok(Self {
            charge,
            storage: self.storage.clone(),
            payload: Box::new(payload),
            ty: self.ty.clone(),
            _owner: self._owner.clone(),
            scope: self.scope.clone(),
            selected: self.selected.clone(),
            revision: self.revision,
        })
    }

    pub(crate) fn payload<S: 'static>(&self) -> NativeResult<&S> {
        self.payload.downcast_ref().ok_or_else(|| {
            RuntimeError::module_validation(
                "native payload access codec differs from the installed storage",
            )
        })
    }

    pub(crate) fn payload_mut<S: 'static>(&mut self) -> NativeResult<&mut S> {
        self.payload.downcast_mut().ok_or_else(|| {
            RuntimeError::module_validation(
                "native payload access codec differs from the installed storage",
            )
        })
    }
}
