//! One traced Rust payload mechanism for native script-heap objects.
//! Factories and Rust type identity are local installation data, never portable ABI.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::{
        arguments::{TypeArgument, type_parameter},
        bindings::TypeBindings,
        compatibility::TypeView,
    },
    gc::GcHeap,
    module::LoadedModule,
    native::{
        binding::NativeResult,
        context::{LinkedCallable, LinkedOperation},
        payload::{data::NativeData, managed::ManagedSchema},
        storage_type::StorageType,
        stored_selection::StoredSelection,
    },
    value::Value,
};
use kagari_common::identity::{reference::DefinitionReference, table::DefinitionId};
use kagari_contract::standard::RuntimePrimitive;
use kagari_types::{declaration::native::NativeStorageLayout, ty::Ty};
use std::{
    any::{Any, TypeId},
    fmt,
    fmt::Debug,
    slice,
    sync::Arc,
};

/// Trace every script value retained by this payload. The visitor cannot execute
/// scripts or retain a reference past tracing. Rust Drop owns payload destruction.
/// The runtime exclusively owns the payload, so Send permits transfer without
/// requiring Sync. Factories shared by multiple installations require Send + Sync.
pub trait NativePayload: Any + Debug + Send {
    fn trace<'payload>(&'payload self, visit: &mut dyn FnMut(&'payload Value));

    /// Managed iterators used by this payload. These are also GC edges. A for
    /// scope keeps their sources protected and releases them on every exit.
    fn iteration_sources<'payload>(&'payload self, _visit: &mut dyn FnMut(&'payload Value)) {}

    /// Logical heap units retained by the payload, excluding its object header.
    fn units(&self) -> usize;
}

pub struct StorageContext<'call> {
    pub(crate) runtime: &'call Runtime,
    pub(crate) owner: &'call LoadedModule,
    pub(crate) ty: &'call Ty<DefinitionId>,
    pub(crate) scope: Option<&'call TypeArgument>,
    pub(crate) selected: &'call [LinkedOperation],
}

impl<'call> StorageContext<'call> {
    pub fn heap(&self) -> &'call GcHeap {
        self.runtime.gc()
    }

    pub fn resolve_type<I: DefinitionReference>(&self, ty: &Ty<I>) -> NativeResult<TypeArgument> {
        self.runtime
            .resolve_type_arguments(self.owner, slice::from_ref(ty))?
            .pop()
            .ok_or_else(|| RuntimeError::module_validation("native storage type scope"))
    }

    pub fn allocate_sequence(
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

    pub fn owner(&self) -> &'call LoadedModule {
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

    pub fn ty(&self) -> &'call Ty<DefinitionId> {
        self.ty
    }
}

type Factory =
    dyn for<'call> Fn(&StorageContext<'call>) -> NativeResult<Box<dyn Any + Send>> + Send + Sync;

type Trace = dyn for<'payload> Fn(&'payload dyn Any, &mut dyn FnMut(&'payload Value)) + Send + Sync;

type Units = dyn Fn(&dyn Any) -> usize + Send + Sync;

struct StorageEntries {
    rust_type: TypeId,
    layout: NativeStorageLayout,
    factory: Option<Box<Factory>>,
    trace: Box<Trace>,
    iteration_sources: Box<Trace>,
    units: Box<Units>,
    editable_data: bool,
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
        let mut storage = Self::payload::<S>();
        Arc::get_mut(&mut storage.entries)
            .expect("new storage descriptor")
            .editable_data = true;
        storage
    }

    pub(crate) fn is_editable_data(&self) -> bool {
        self.entries.editable_data
    }

    pub(crate) fn accepts_payload<S: NativePayload>(&self) -> bool {
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
                iteration_sources: Box::new(|payload, visit| {
                    payload
                        .downcast_ref::<S>()
                        .expect("sealed Rust payload type")
                        .iteration_sources(visit)
                }),
                units: Box::new(|payload| {
                    payload
                        .downcast_ref::<S>()
                        .expect("factory and size entries share the sealed Rust payload type")
                        .units()
                }),
                editable_data: false,
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
        ty: &Ty<DefinitionId>,
        payload: S,
        owner: &LoadedModule,
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
        ty: &Ty<DefinitionId>,
        payload: Box<dyn Any + Send>,
        owner: &LoadedModule,
    ) -> NativeResult<NativeObject> {
        let mut valid = true;
        (self.entries.trace)(payload.as_ref(), &mut |value| {
            valid &= value.is_default_heap_payload() && heap.validate_value(value);
        });
        (self.entries.iteration_sources)(payload.as_ref(), &mut |value| {
            valid &= value.is_default_heap_payload() && heap.validate_value(value);
        });
        if !valid {
            return Err(RuntimeError::module_validation(
                "native factory retained an invalid script value",
            ));
        }
        Ok(NativeObject {
            storage: self.clone(),
            payload,
            ty: ty.clone(),
            _owner: owner.clone(),
            scope: None,
            selected: Arc::from([]),
        })
    }
}

/// GC owns the payload and immutable type metadata. Only actual executable
/// dependencies (stored callbacks or selected key operations) retain module instances;
/// data alone must not
/// create a module-state -> data -> module-instance retention cycle.
pub(crate) struct NativeObject {
    pub(crate) storage: NativeStorage,
    payload: Box<dyn Any + Send>,
    pub(crate) ty: Ty<DefinitionId>,
    _owner: LoadedModule,
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
    // Linking compares every native storage contract with its installed owner.
    // Installation forbids replacing that owner while its heap objects exist.
    pub(crate) fn definition_name(&self, id: DefinitionId) -> Option<&str> {
        self._owner.definition_name(id)
    }

    pub(crate) fn matches(
        &self,
        ty: &Ty<DefinitionId>,
        owner: &LoadedModule,
        environment: Option<&TypeBindings>,
    ) -> bool {
        let actual = match &self.scope {
            Some(scope) => scope.view(&self._owner),
            None => TypeView::new(&self.ty, &self._owner, None),
        };
        actual.compatible(TypeView::new(ty, owner, environment))
    }

    pub(crate) fn trace<'payload>(&'payload self, visit: &mut dyn FnMut(&'payload Value)) {
        (self.storage.entries.trace)(self.payload.as_ref(), visit);
        self.iteration_sources(visit);
    }

    pub(crate) fn iteration_sources<'payload>(
        &'payload self,
        visit: &mut dyn FnMut(&'payload Value),
    ) {
        (self.storage.entries.iteration_sources)(self.payload.as_ref(), visit);
    }

    pub(crate) fn units(&self) -> usize {
        (self.storage.entries.units)(self.payload.as_ref())
    }

    pub(crate) fn replaced_payload<S: NativePayload>(&self, payload: S) -> NativeResult<Self> {
        if self.storage.entries.rust_type != TypeId::of::<S>() {
            return Err(RuntimeError::module_validation(
                "native replacement payload type mismatch",
            ));
        }
        Ok(Self {
            storage: self.storage.clone(),
            payload: Box::new(payload),
            ty: self.ty.clone(),
            _owner: self._owner.clone(),
            scope: self.scope.clone(),
            selected: self.selected.clone(),
        })
    }

    pub(crate) fn payload<S: NativePayload>(&self) -> NativeResult<&S> {
        self.payload.downcast_ref().ok_or_else(|| {
            RuntimeError::module_validation(
                "native payload access codec differs from the installed storage",
            )
        })
    }

    pub(crate) fn payload_mut<S: NativePayload>(&mut self) -> NativeResult<&mut S> {
        self.payload.downcast_mut().ok_or_else(|| {
            RuntimeError::module_validation(
                "native payload access codec differs from the installed storage",
            )
        })
    }
}
