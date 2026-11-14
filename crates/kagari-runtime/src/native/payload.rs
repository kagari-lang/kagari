//! Retained access to registered, exclusively runtime-owned Rust payloads.
mod binding;
mod conversion;
pub mod data;
pub mod managed;
mod methods;

use crate::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    module::ExecutableModule,
    native::{
        binding::NativeResult,
        conversion::context::{ConversionContext, native::NativeContract},
        payload::{data::NativeData, managed::AppliedSchema},
        rust_type::RustType,
        storage::{NativePayload, NativeStorage, initializer::NativeInit},
        typed::NativeContext,
    },
    value::{carriers::RootedValue, view::ValueView},
};
use kagari_gc::{
    error::GcError,
    object::{Gc, GcObject},
};
use std::{fmt::Debug, marker::PhantomData, sync::Arc};

#[derive(Debug)]
struct TypeRecord<T> {
    rust_type: RustType<T>,
    storage: NativeStorage,
    managed: Option<Arc<AppliedSchema>>,
}

/// A prepared native storage application. Clones reuse its checked descriptor.
#[derive(Debug)]
pub struct NativeType<T> {
    record: Arc<TypeRecord<T>>,
    mapping: PhantomData<fn() -> T>,
}

impl<T> Clone for NativeType<T> {
    fn clone(&self) -> Self {
        Self {
            record: self.record.clone(),
            mapping: PhantomData,
        }
    }
}

/// An owning root, never an exposed Rust pointer. Reading lends the payload only
/// for the closure; nested execution and conflicting access fail structurally.
#[derive(Debug)]
pub struct Opaque<T> {
    root: RootedValue,
    native_type: NativeType<T>,
}

impl<T> Clone for Opaque<T> {
    fn clone(&self) -> Self {
        Self {
            root: self.root.clone(),
            native_type: self.native_type.clone(),
        }
    }
}

impl<T> NativeType<T> {
    /// The same checked target descriptor used when lending external Rust storage.
    pub fn rust_type(&self) -> &RustType<T> {
        &self.record.rust_type
    }

    pub fn type_argument(&self) -> &TypeArgument {
        self.record.rust_type.type_argument()
    }

    pub fn owner(&self) -> &ExecutableModule {
        self.record.rust_type.owner()
    }
}

impl<T> NativeType<T> {
    fn contract(&self) -> NativeContract<'_> {
        NativeContract {
            storage: &self.record.storage,
            owner: self.owner(),
            applied: self.type_argument(),
        }
    }
}

impl<T: NativeData> NativeType<T> {
    /// Construct recursively checked fixed Rust data. No script edge can be
    /// detached from its source during preparation or payload allocation.
    pub fn create(&self, cx: &mut NativeContext<'_>, data: T) -> NativeResult<Opaque<T>> {
        let root = cx.conversion.create_native_data(self.contract(), data)?;
        cx.runtime_mut().gc_safepoint()?;
        Ok(Opaque {
            root,
            native_type: self.clone(),
        })
    }
}

impl<T: GcObject + Debug> NativeType<T> {
    /// Construct a concrete provider inside one noncollecting branded interval.
    /// The callback uses its provider's reviewed constructors (or the unsafe GC
    /// initializer protocol). It cannot collect/reenter this Runtime or return a
    /// handle from another heap. Only the completed Native identity is retained.
    /// Finish conversion and cooperative long preparation before this callback;
    /// its GC initialization/publication interval is indivisible.
    pub fn create_with(
        &self,
        cx: &mut NativeContext<'_>,
        initialize: impl for<'loan, 'source, 'gc> FnOnce(
            &mut NativeInit<'loan, 'source, 'gc>,
        ) -> Result<Gc<'gc, T>, GcError>,
    ) -> NativeResult<Opaque<T>> {
        let root = cx
            .conversion
            .create_native_with(self.contract(), initialize)?;
        cx.runtime_mut().gc_safepoint()?;
        Ok(Opaque {
            root,
            native_type: self.clone(),
        })
    }

    pub(crate) fn replace_with(
        &self,
        cx: &mut NativeContext<'_>,
        target: &Opaque<T>,
        initialize: impl for<'loan, 'source, 'gc> FnOnce(
            &mut NativeInit<'loan, 'source, 'gc>,
        ) -> Result<Gc<'gc, T>, GcError>,
    ) -> NativeResult<()> {
        cx.conversion
            .replace_native_with(&target.root, self.contract(), initialize)?;
        cx.runtime_mut().gc_safepoint()
    }
}

impl<T: Send + 'static> Opaque<T> {
    pub fn native_type(&self) -> &NativeType<T> {
        &self.native_type
    }

    /// Lend the provider only for this closed, non-reentrant operation.
    /// Completed effects survive an error or unwind; a payload reference cannot
    /// escape and Runtime services cannot overlap the loan.
    ///
    /// ```compile_fail
    /// use kagari_runtime::native::{payload::Opaque, typed::NativeContext};
    /// fn escape<'a>(object: &Opaque<i32>, cx: &'a mut NativeContext<'_>) -> &'a i32 {
    ///     object.read(cx, |value| Ok(value)).unwrap()
    /// }
    /// ```
    /// ```compile_fail,E0500
    /// use kagari_runtime::native::{payload::Opaque, typed::NativeContext};
    /// fn reenter(object: &Opaque<i32>, cx: &mut NativeContext<'_>) {
    ///     object.read(cx, |_| { cx.collect_garbage()?; Ok(()) }).unwrap();
    /// }
    /// ```
    pub fn read<R>(
        &self,
        cx: &mut NativeContext<'_>,
        read: impl for<'payload> FnOnce(&'payload T) -> NativeResult<R>,
    ) -> NativeResult<R> {
        cx.conversion
            .read_native_root(&self.root, self.native_type.contract(), read)
    }

    pub(crate) fn edit_checked<R>(
        &self,
        cx: &mut NativeContext<'_>,
        edit: impl for<'payload> FnOnce(&'payload mut T) -> NativeResult<R>,
    ) -> NativeResult<R> {
        cx.conversion
            .edit_native_root(&self.root, self.native_type.contract(), edit)
    }
}

impl<T: NativeData> Opaque<T> {
    /// Lend checked fixed data for one non-reentrant edit. Completed scalar writes
    /// remain visible when the closure fails or unwinds. A reference cannot escape.
    ///
    /// ```compile_fail
    /// use kagari_runtime::native::{payload::Opaque, typed::NativeContext};
    /// fn escape<'a>(object: &Opaque<i32>, cx: &'a mut NativeContext<'_>) -> &'a mut i32 {
    ///     object.edit(cx, |value| Ok(value)).unwrap()
    /// }
    /// ```
    /// Executing/collecting services cannot be borrowed while the payload is lent:
    /// ```compile_fail,E0500
    /// use kagari_runtime::native::{payload::Opaque, typed::NativeContext};
    /// fn reenter(object: &Opaque<i32>, cx: &mut NativeContext<'_>) {
    ///     object.edit(cx, |_| { cx.collect_garbage()?; Ok(()) }).unwrap();
    /// }
    /// ```
    pub fn edit<R>(
        &self,
        cx: &mut NativeContext<'_>,
        edit: impl for<'payload> FnOnce(&'payload mut T) -> NativeResult<R>,
    ) -> NativeResult<R> {
        self.edit_checked(cx, edit)
    }
}

impl NativeContext<'_> {
    /// A typed native constructor can use its declared result application,
    /// including generic scopes, without capturing an installed runtime handle.
    pub fn create_opaque<T: NativeData>(&mut self, payload: T) -> NativeResult<Opaque<T>> {
        self.result_native_type()?.create(self, payload)
    }

    /// Prepare the callback's declared native result for a managed field builder.
    pub fn result_native_type<T: NativePayload>(&mut self) -> NativeResult<NativeType<T>> {
        let function = self.function.ok_or_else(|| {
            RuntimeError::module_validation("native construction requires a declared result")
        })?;
        let argument = function.type_signature()?.result.clone();
        let owner = self.conversion.owner();
        let origin = self.conversion.owner_origin();
        self.runtime_mut()
            .prepare_native_type::<T>(owner, argument, origin)
    }
}

impl<T> Opaque<T> {
    fn conversion<'call>(
        &self,
        cx: &mut ConversionContext<'call>,
        expected: &TypeArgument,
    ) -> NativeResult<ValueView<'call>> {
        if !self
            .native_type
            .type_argument()
            .view(self.native_type.owner())
            .compatible(expected.view(cx.owner()))
        {
            return Err(RuntimeError::module_validation(
                "native object type scope mismatch",
            ));
        }
        cx.import(&self.root)
    }
}
