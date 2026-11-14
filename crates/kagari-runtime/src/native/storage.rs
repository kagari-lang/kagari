//! Installed Rust payload codecs use concrete GC providers, never old heap IDs.
pub mod initializer;
use crate::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    module::ExecutableModule,
    native::{
        binding::NativeResult,
        context::{LinkedCallable, LinkedOperation},
        conversion::context::{ConversionContext, native::NativeContract},
        payload::{data::NativeData, managed::ManagedSchema},
    },
    value::{native::data::DataObject, view::ValueView},
};
use kagari_gc::{
    access::Scope,
    error::GcError,
    object::{AnyObject, Gc, GcObject},
};
use kagari_types::{declaration::native::NativeStorageLayout, ty::Ty};
use std::{
    any::{Any, TypeId},
    fmt::{self, Debug},
    slice,
    sync::Arc,
    thread,
};

/// Automatic schema-registration policy for an owned Rust mapping. This is not a
/// tracing callback: concrete providers implement GcObject's unsafe contract.
/// Fixed data uses its recursively checked NativeData representation instead.
pub trait NativePayload: Any + Debug + Send {
    fn registration_storage() -> NativeStorage
    where
        Self: Sized;
}

impl<T: NativeData> NativePayload for T {
    fn registration_storage() -> NativeStorage {
        NativeStorage::data::<T>()
    }
}

pub struct StorageContext<'scope, 'call> {
    pub(crate) conversion: &'scope mut ConversionContext<'call>,
    pub(crate) owner: &'scope ExecutableModule,
    pub(crate) applied: &'scope TypeArgument,
    pub(crate) selected: &'scope [LinkedOperation],
}

impl<'scope, 'call> StorageContext<'scope, 'call> {
    /// Cooperatively poll while preparing a factory's owned fixed data.
    pub fn poll(&self) -> NativeResult<()> {
        self.conversion.poll()
    }

    pub fn resolve_type(&self, ty: &Ty) -> NativeResult<TypeArgument> {
        self.conversion
            .runtime()
            .resolve_type_arguments(self.owner, slice::from_ref(ty))?
            .pop()
            .ok_or_else(|| RuntimeError::module_validation("native storage type scope"))
    }

    pub fn allocate_array(
        &mut self,
        element: TypeArgument,
        elements: &[ValueView<'_>],
    ) -> NativeResult<ValueView<'call>> {
        self.conversion.allocate_array(&element, elements)
    }

    pub fn type_parameter(&self, index: usize) -> NativeResult<TypeArgument> {
        self.applied
            .parameter(self.conversion.runtime(), self.owner, index)
    }

    pub fn owner(&self) -> &'scope ExecutableModule {
        self.owner
    }

    pub fn selected(&self, slot: usize) -> NativeResult<&'scope LinkedCallable> {
        self.selected
            .get(slot)
            .and_then(LinkedOperation::ready)
            .ok_or_else(|| {
                RuntimeError::module_validation("native storage requires a prepared callable slot")
            })
    }

    pub fn ty(&self) -> &'scope Ty {
        self.applied.ty()
    }
}

type Factory = dyn for<'scope, 'call> Fn(&mut StorageContext<'scope, 'call>) -> NativeResult<Box<dyn Any + Send>>
    + Send
    + Sync;
type AllocateData =
    for<'gc> fn(&mut Scope<'gc>, Box<dyn Any + Send>) -> Result<Gc<'gc, AnyObject>, GcError>;
type Read =
    for<'gc> fn(&Scope<'gc>, Gc<'gc, AnyObject>, &mut dyn FnMut(&dyn Any)) -> Result<(), GcError>;
type EditData = for<'gc> fn(
    &mut Scope<'gc>,
    Gc<'gc, AnyObject>,
    &mut dyn FnMut(&mut dyn Any),
) -> Result<thread::Result<()>, GcError>;

pub(crate) struct PayloadDescriptor {
    rust_type: TypeId,
    layout: NativeStorageLayout,
    allocate_data: Option<AllocateData>,
    read: Read,
    edit_data: Option<EditData>,
    managed: Option<Arc<ManagedSchema>>,
}

impl PayloadDescriptor {
    pub(crate) fn layout(&self) -> NativeStorageLayout {
        self.layout
    }

    pub(crate) fn matches_payload<S: 'static>(&self) -> bool {
        self.rust_type == TypeId::of::<S>()
    }

    /// The actual Native header retains this exact physical codec. Reading an
    /// old admitted object does not resolve a replacement registration on reentry.
    pub(crate) fn read_payload<'gc, S: 'static, R>(
        &self,
        cx: &Scope<'gc>,
        value: Gc<'gc, AnyObject>,
        read: impl for<'payload> FnOnce(&'payload S) -> NativeResult<R>,
    ) -> Result<NativeResult<R>, GcError> {
        if self.rust_type != TypeId::of::<S>() {
            return Err(GcError::Representation);
        }
        let mut read = Some(read);
        let mut output = None;
        (self.read)(cx, value, &mut |payload| {
            output = Some(match payload.downcast_ref::<S>() {
                Some(payload) => read.take().expect("one sealed payload read")(payload),
                None => Err(RuntimeError::module_validation("native payload codec")),
            });
        })?;
        Ok(output.expect("sealed payload read called once"))
    }
}

impl Debug for PayloadDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PayloadDescriptor")
            .field("layout", &self.layout)
            .field("rust_type", &self.rust_type)
            .finish_non_exhaustive()
    }
}

struct StorageEntries {
    descriptor: Arc<PayloadDescriptor>,
    factory: Option<Box<Factory>>,
}

/// One immutable Rust-type/physical-access proof, shared by installed mappings.
#[derive(Clone)]
pub struct NativeStorage {
    entries: Arc<StorageEntries>,
}

impl Debug for NativeStorage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NativeStorage")
            .field("layout", &self.entries.descriptor.layout)
            .field("rust_type", &self.entries.descriptor.rust_type)
            .finish_non_exhaustive()
    }
}

impl NativeStorage {
    pub(crate) fn with_managed_schema(mut self, schema: ManagedSchema) -> Self {
        Arc::get_mut(
            &mut Arc::get_mut(&mut self.entries)
                .expect("new storage entries")
                .descriptor,
        )
        .expect("new payload descriptor")
        .managed = Some(Arc::new(schema));
        self
    }

    pub(crate) fn managed_schema(&self) -> Option<&Arc<ManagedSchema>> {
        self.entries.descriptor.managed.as_ref()
    }

    pub fn data<S: NativeData>() -> Self {
        Self {
            entries: Arc::new(StorageEntries {
                descriptor: Arc::new(PayloadDescriptor {
                    rust_type: TypeId::of::<S>(),
                    layout: NativeStorageLayout::Opaque,
                    allocate_data: Some(|cx, value| {
                        let value = value.downcast::<S>().map_err(|_| GcError::Representation)?;
                        DataObject::alloc(cx, *value).map(Gc::erase)
                    }),
                    read: |cx, value, read| {
                        let value = cx.downcast::<DataObject<S>>(value)?;
                        cx.read(value, |value| read(value.value()));
                        Ok(())
                    },
                    edit_data: Some(|cx, value, edit| {
                        let value = cx.downcast::<DataObject<S>>(value)?;
                        DataObject::edit(cx, value, |value| edit(value))
                    }),
                    managed: None,
                }),
                factory: None,
            }),
        }
    }

    pub fn new<S: NativeData>(
        factory: impl for<'scope, 'call> Fn(&mut StorageContext<'scope, 'call>) -> NativeResult<S>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        let mut storage = Self::data::<S>();
        Arc::get_mut(&mut storage.entries)
            .expect("new storage descriptor")
            .factory = Some(Box::new(move |context| {
            factory(context).map(|value| Box::new(value) as Box<dyn Any + Send>)
        }));
        storage
    }

    /// A concrete traced provider supplies its own scanner, charge and destructor.
    /// Construction uses a branded NativeInit and the provider's reviewed API;
    /// no safe Runtime callback accepts detached owned GC edges as input.
    pub fn payload<S: GcObject + Debug>() -> Self {
        Self::provided_with_layout::<S>(NativeStorageLayout::Opaque)
    }

    pub(crate) fn provided_with_layout<S: GcObject + Debug>(layout: NativeStorageLayout) -> Self {
        Self {
            entries: Arc::new(StorageEntries {
                descriptor: Arc::new(PayloadDescriptor {
                    rust_type: TypeId::of::<S>(),
                    layout,
                    allocate_data: None,
                    read: |cx, value, read| {
                        let value = cx.downcast::<S>(value)?;
                        cx.read(value, |value| read(value));
                        Ok(())
                    },
                    edit_data: None,
                    managed: None,
                }),
                factory: None,
            }),
        }
    }

    pub(crate) fn same_registration(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.entries.descriptor, &other.entries.descriptor)
    }

    pub(crate) fn descriptor(&self) -> &Arc<PayloadDescriptor> {
        &self.entries.descriptor
    }

    pub(crate) fn matches_descriptor(&self, descriptor: &Arc<PayloadDescriptor>) -> bool {
        Arc::ptr_eq(&self.entries.descriptor, descriptor)
    }

    pub fn layout(&self) -> NativeStorageLayout {
        self.entries.descriptor.layout
    }

    pub(crate) fn is_editable_data(&self) -> bool {
        self.entries.descriptor.edit_data.is_some()
    }

    pub(crate) fn accepts_payload<S: 'static>(&self) -> bool {
        self.entries.descriptor.rust_type == TypeId::of::<S>()
    }

    pub(crate) fn accepts_traced<S: GcObject>(&self) -> bool {
        self.accepts_payload::<S>() && self.entries.descriptor.allocate_data.is_none()
    }

    pub(crate) fn allocate_data<'gc>(
        &self,
        cx: &mut Scope<'gc>,
        value: Box<dyn Any + Send>,
    ) -> Result<Gc<'gc, AnyObject>, GcError> {
        if value.as_ref().type_id() != self.entries.descriptor.rust_type {
            return Err(GcError::Representation);
        }
        self.entries
            .descriptor
            .allocate_data
            .ok_or(GcError::Representation)?(cx, value)
    }

    pub(crate) fn read<'gc, S: 'static, R>(
        &self,
        cx: &Scope<'gc>,
        value: Gc<'gc, AnyObject>,
        read: impl for<'payload> FnOnce(&'payload S) -> NativeResult<R>,
    ) -> Result<NativeResult<R>, GcError> {
        self.entries.descriptor.read_payload(cx, value, read)
    }

    pub(crate) fn edit_data<'gc, S: 'static, R>(
        &self,
        cx: &mut Scope<'gc>,
        value: Gc<'gc, AnyObject>,
        edit: impl for<'payload> FnOnce(&'payload mut S) -> NativeResult<R>,
    ) -> Result<thread::Result<NativeResult<R>>, GcError> {
        if !self.accepts_payload::<S>() {
            return Err(GcError::Representation);
        }
        let entry = self
            .entries
            .descriptor
            .edit_data
            .ok_or(GcError::Representation)?;
        let mut edit = Some(edit);
        let mut output = None;
        let result = entry(cx, value, &mut |payload| {
            output = Some(match payload.downcast_mut::<S>() {
                Some(payload) => edit.take().expect("one sealed payload edit")(payload),
                None => Err(RuntimeError::module_validation("native payload codec")),
            });
        })?;
        Ok(result.map(|()| output.expect("sealed payload edit called once")))
    }

    pub(crate) fn create<'call>(
        &self,
        context: &mut StorageContext<'_, 'call>,
    ) -> NativeResult<ValueView<'call>> {
        let factory = self.entries.factory.as_ref().ok_or_else(|| {
            RuntimeError::module_validation(
                "native storage requires an explicit provider constructor",
            )
        })?;
        let owner = context.owner;
        let applied = context.applied;
        let selected = context.selected;
        let contract = NativeContract {
            storage: self,
            owner,
            applied,
        };
        context
            .conversion
            .allocate_native_factory(contract, |conversion| {
                factory(&mut StorageContext {
                    conversion,
                    owner,
                    applied,
                    selected,
                })
            })
    }
}
