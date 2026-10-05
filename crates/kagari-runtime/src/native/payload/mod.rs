//! Retained access to registered, exclusively runtime-owned Rust payloads.
mod binding;
mod conversion;
pub mod data;
pub mod managed;
mod methods;

use crate::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    gc::{GcHeap, roots::RootedValue},
    module::{LoadedModule, retention::ProgramLease},
    native::{
        binding::NativeResult,
        conversion::context::ConversionContext,
        payload::{data::NativeData, managed::AppliedSchema},
        storage::{NativePayload, NativeStorage},
        typed::NativeContext,
    },
    value::Value,
};
use std::{marker::PhantomData, slice, sync::Arc};

#[derive(Debug)]
struct TypeRecord {
    argument: TypeArgument,
    owner: LoadedModule,
    storage: NativeStorage,
    managed: Option<Arc<AppliedSchema>>,
    _program: ProgramLease,
}

/// A prepared native storage application. Clones reuse its checked descriptor.
#[derive(Debug)]
pub struct NativeType<T> {
    record: Arc<TypeRecord>,
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
pub struct NativeObject<T> {
    root: RootedValue,
    native_type: NativeType<T>,
}

impl<T> Clone for NativeObject<T> {
    fn clone(&self) -> Self {
        Self {
            root: self.root.clone(),
            native_type: self.native_type.clone(),
        }
    }
}

impl<T> NativeType<T> {
    pub fn type_argument(&self) -> &TypeArgument {
        &self.record.argument
    }

    pub fn owner(&self) -> &LoadedModule {
        &self.record.owner
    }
}

impl<T: NativePayload> NativeType<T> {
    /// Construct from the registered payload representation. Handwritten
    /// NativePayload implementations remain the low-level tracing boundary.
    pub fn create(&self, cx: &mut NativeContext<'_>, payload: T) -> NativeResult<NativeObject<T>> {
        let runtime = cx.runtime();
        runtime.gc().ensure_no_native_borrow()?;
        runtime.resources().ensure_execution_allowed()?;
        runtime.validate_loaded_module(self.owner())?;
        self.type_argument().validate(runtime)?;
        cx.poll()?;
        let mut object = self.record.storage.prepare_payload(
            runtime.gc(),
            self.type_argument().ty(),
            payload,
            self.owner(),
        )?;
        object.scope = Some(self.type_argument().clone());
        let id = runtime.gc().alloc_native(object)?;
        let root = runtime
            .root_value(Value::GcHandle(id))
            .ok_or_else(|| RuntimeError::module_validation("native object retention"))?;
        runtime.gc_safepoint()?;
        Ok(NativeObject {
            root,
            native_type: self.clone(),
        })
    }
}

impl<T: NativePayload> NativeObject<T> {
    pub fn native_type(&self) -> &NativeType<T> {
        &self.native_type
    }

    pub fn read<R>(
        &self,
        cx: &NativeContext<'_>,
        read: impl for<'payload> FnOnce(&'payload T) -> NativeResult<R>,
    ) -> NativeResult<R> {
        cx.runtime().gc().ensure_no_native_borrow()?;
        cx.runtime().resources().ensure_execution_allowed()?;
        cx.poll()?;
        let value = self.value(cx.runtime().gc())?;
        let Value::GcHandle(id) = value else {
            unreachable!("native object root")
        };
        cx.runtime().gc().with_native(id, read)
    }
}

impl<T: NativeData> NativeObject<T> {
    /// Lend checked fixed data for one non-reentrant edit. Completed scalar writes
    /// remain visible when the closure fails or unwinds. A reference cannot escape.
    ///
    /// ```compile_fail
    /// use kagari_runtime::native::{payload::NativeObject, typed::NativeContext};
    /// fn escape<'a>(object: &NativeObject<i32>, cx: &'a NativeContext<'_>) -> &'a mut i32 {
    ///     object.edit(cx, |value| Ok(value)).unwrap()
    /// }
    /// ```
    pub fn edit<R>(
        &self,
        cx: &NativeContext<'_>,
        edit: impl for<'payload> FnOnce(&'payload mut T) -> NativeResult<R>,
    ) -> NativeResult<R> {
        cx.runtime().gc().ensure_no_native_borrow()?;
        cx.runtime().resources().ensure_execution_allowed()?;
        cx.poll()?;
        let Value::GcHandle(id) = self.value(cx.runtime().gc())? else {
            unreachable!("native object root")
        };
        cx.runtime().gc().with_native_data_mut(id, edit)
    }
}

impl NativeContext<'_> {
    /// A typed native constructor can use its declared result application,
    /// including generic scopes, without capturing an installed runtime handle.
    pub fn create_native<T: NativePayload>(&mut self, payload: T) -> NativeResult<NativeObject<T>> {
        self.result_native_type()?.create(self, payload)
    }

    /// Prepare the callback's declared native result for a managed field builder.
    pub fn result_native_type<T: NativePayload>(&self) -> NativeResult<NativeType<T>> {
        let function = self.function.ok_or_else(|| {
            RuntimeError::module_validation("native construction requires a declared result")
        })?;
        let argument = match &function.scoped_signature {
            Some(signature) => signature.result.clone(),
            None => self
                .runtime()
                .type_arguments(
                    self.conversion.owner(),
                    None,
                    slice::from_ref(&function.signature.result),
                )?
                .pop()
                .ok_or_else(|| RuntimeError::module_validation("native result type scope"))?,
        };
        self.runtime()
            .prepare_native_type::<T>(self.conversion.owner(), argument)
    }
}

impl<T> NativeObject<T> {
    fn value(&self, heap: &GcHeap) -> NativeResult<Value> {
        self.root
            .value(heap)
            .ok_or_else(|| RuntimeError::module_validation("foreign or expired native object"))
    }

    fn conversion(
        &self,
        cx: &ConversionContext<'_>,
        expected: &TypeArgument,
    ) -> NativeResult<Value> {
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
        self.value(cx.runtime().gc())
    }
}
