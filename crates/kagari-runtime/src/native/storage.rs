//! One traced Rust payload mechanism for native script-heap objects.
//! Factories and Rust type identity are local installation data, never portable ABI.
use crate::{
    Runtime,
    error::RuntimeError,
    gc::GcHeap,
    module::LoadedModule,
    native::{binding::NativeResult, context::LinkedCallable},
    value::Value,
};
use kagari_abi::types::{AbiType, native::NativeStorageLayout};
use std::{
    any::{Any, TypeId},
    fmt::{self, Debug},
    rc::Rc,
};

/// Trace every script value retained by this payload. The visitor cannot execute
/// scripts or retain a reference past tracing. Rust Drop owns payload destruction.
pub trait NativePayload: Any + Debug {
    fn trace<'payload>(&'payload self, visit: &mut dyn FnMut(&'payload Value));
    /// Logical heap units retained by the payload, excluding its object header.
    fn units(&self) -> usize;
}

pub struct StorageContext<'call> {
    pub(crate) runtime: &'call Runtime,
    pub(crate) owner: &'call LoadedModule,
    pub(crate) ty: &'call AbiType,
    pub(crate) selected: &'call [LinkedCallable],
}
impl<'call> StorageContext<'call> {
    pub fn heap(&self) -> &'call GcHeap {
        self.runtime.gc()
    }
    pub fn allocate_sequence(&self, element: AbiType, elements: Vec<Value>) -> NativeResult<Value> {
        self.runtime
            .alloc_array(self.owner, element, elements)
            .map(Value::Array)
    }
    pub fn owner(&self) -> &'call LoadedModule {
        self.owner
    }
    pub fn selected(&self, slot: usize) -> NativeResult<&'call LinkedCallable> {
        self.selected.get(slot).ok_or_else(|| {
            RuntimeError::module_validation("native storage requires a prepared callable slot")
        })
    }
    pub fn ty(&self) -> &'call AbiType {
        self.ty
    }
}

type Factory = dyn for<'call> Fn(&StorageContext<'call>) -> NativeResult<Box<dyn Any>>;
type Trace = dyn for<'payload> Fn(&'payload dyn Any, &mut dyn FnMut(&'payload Value));
type Units = dyn Fn(&dyn Any) -> usize;

struct StorageEntries {
    rust_type: TypeId,
    layout: NativeStorageLayout,
    factory: Option<Box<Factory>>,
    trace: Box<Trace>,
    units: Box<Units>,
}

/// An immutable checked erasure of one Rust payload type. Clone shares entries.
#[derive(Clone)]
pub struct NativeStorage {
    entries: Rc<StorageEntries>,
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
    pub fn new<S: NativePayload>(
        factory: impl for<'call> Fn(&StorageContext<'call>) -> NativeResult<S> + 'static,
    ) -> Self {
        Self::with_layout(NativeStorageLayout::Opaque, factory)
    }
    pub(crate) fn with_layout<S: NativePayload>(
        layout: NativeStorageLayout,
        factory: impl for<'call> Fn(&StorageContext<'call>) -> NativeResult<S> + 'static,
    ) -> Self {
        Self::entries::<S>(
            layout,
            Some(Box::new(move |context| {
                factory(context).map(|payload| Box::new(payload) as Box<dyn Any>)
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
            entries: Rc::new(StorageEntries {
                rust_type: TypeId::of::<S>(),
                layout,
                factory,
                trace: Box::new(|payload, visit| {
                    payload
                        .downcast_ref::<S>()
                        .expect("factory and trace entries share the sealed Rust payload type")
                        .trace(visit)
                }),
                units: Box::new(|payload| {
                    payload
                        .downcast_ref::<S>()
                        .expect("factory and size entries share the sealed Rust payload type")
                        .units()
                }),
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
        self.object(context.heap(), context.ty, payload, context.owner)
    }
    pub(crate) fn prepare_payload<S: NativePayload>(
        &self,
        heap: &GcHeap,
        ty: &AbiType,
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
        ty: &AbiType,
        payload: Box<dyn Any>,
        owner: &LoadedModule,
    ) -> NativeResult<NativeObject> {
        let mut valid = true;
        (self.entries.trace)(payload.as_ref(), &mut |value| {
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
        })
    }
}

/// GC owns the payload and immutable type metadata. Only actual executable
/// values (such as stored callbacks) retain module instances; data alone must not
/// create a module-state -> data -> module-instance retention cycle.
pub(crate) struct NativeObject {
    pub(crate) storage: NativeStorage,
    payload: Box<dyn Any>,
    pub(crate) ty: AbiType,
    _owner: LoadedModule,
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
    pub(crate) fn matches(&self, ty: &AbiType) -> bool {
        self.ty == *ty
    }
    pub(crate) fn trace<'payload>(&'payload self, visit: &mut dyn FnMut(&'payload Value)) {
        (self.storage.entries.trace)(self.payload.as_ref(), visit);
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
