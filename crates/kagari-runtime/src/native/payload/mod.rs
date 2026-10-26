//! Retained access to registered, exclusively runtime-owned Rust payloads.
mod binding;
mod conversion;
pub mod data;
pub mod managed;
mod methods;
pub mod take;

use crate::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    gc::{GcHeap, roots::RootedValue},
    module::ExecutableModule,
    native::{
        binding::NativeResult,
        conversion::{KagariType, arguments::KagariArguments, context::ConversionContext},
        function_handle::Function,
        payload::{data::NativeData, managed::AppliedSchema, take::NativeTake},
        rust_type::RustType,
        storage::{
            NativePayload, NativeStorage, StorageContext,
            access::{EditPlan, StorageAccess},
        },
        stored_selection::StoredSelection,
        typed::NativeContext,
        views::{NativeOperationGuard, NativeOperationKind, Recovery},
    },
    value::Value,
};
use std::{marker::PhantomData, sync::Arc};

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

impl<T: NativePayload> NativeType<T> {
    /// Construct from the registered payload representation. Handwritten
    /// NativePayload implementations remain the low-level tracing boundary.
    pub fn create(
        &self,
        cx: &mut NativeContext<'_>,
        payload: T,
        selected_slots: &[usize],
    ) -> NativeResult<Opaque<T>> {
        let runtime = cx.runtime();
        runtime.gc().ensure_execution_allowed()?;
        let selected = if selected_slots.is_empty() {
            Arc::from([])
        } else {
            let function = cx.function.ok_or_else(|| {
                RuntimeError::module_validation(
                    "selected construction requires native call evidence",
                )
            })?;
            StoredSelection::slots(cx.conversion.owner(), &function.selected, selected_slots)?
        };
        let root = self.record.storage.allocate_payload(
            &StorageContext {
                runtime,
                owner: self.owner(),
                ty: self.type_argument().ty(),
                scope: Some(self.type_argument()),
                selected: &[],
            },
            payload,
            selected,
        )?;
        Ok(Opaque {
            root,
            native_type: self.clone(),
        })
    }
}

impl<T: Send + 'static> Opaque<T> {
    pub fn native_type(&self) -> &NativeType<T> {
        &self.native_type
    }

    /// Retain a typed relationship from this object's selected metadata. Each
    /// ordinary call selects its current body; prepare once to pin an active computation.
    pub fn stored_function<A: KagariArguments, R: KagariType>(
        &self,
        cx: &NativeContext<'_>,
        slot: usize,
    ) -> NativeResult<Function<A, R>> {
        let Value::GcHandle(id) = self.value(cx.runtime().gc())? else {
            unreachable!("opaque root")
        };
        let selections = cx.runtime().gc().native_selections(id)?;
        selections
            .get(slot)
            .ok_or_else(|| RuntimeError::module_validation("stored callable slot"))?
            .function(cx.runtime())
    }

    pub(crate) fn read_checked<R>(
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
        cx.runtime().gc().check_native_payload::<T>(id, true)?;
        cx.runtime().gc().with_native(id, read)
    }

    pub(crate) fn edit_checked<R>(
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

impl<T: NativePayload> Opaque<T> {
    /// Lend provider storage without allowing script reentry.
    ///
    /// # Safety
    /// Retain every extracted edge before it leaves the loan. Interior writes must
    /// not change traced edges, structural state or installed accounting. Read-only
    /// operation protection also forbids interior scalar writes to the receiver.
    pub unsafe fn read<R>(
        &self,
        cx: &NativeContext<'_>,
        operation: Option<&NativeOperationGuard<'_>>,
        read: impl for<'a> FnOnce(&'a T, &mut StorageAccess<'a>) -> NativeResult<R>,
    ) -> NativeResult<R> {
        cx.poll()?;
        let Value::GcHandle(id) = self.value(cx.runtime().gc())? else {
            unreachable!("opaque root")
        };
        cx.runtime()
            .gc()
            .read_native(cx.runtime(), id, operation, read)
    }

    /// Edit provider storage with preflighted accounting and unwind reconciliation.
    ///
    /// # Safety
    /// Publish imported edges through StorageAccess, keep all work state traced in
    /// this payload, and respect the plan's maximum charge and structural contract.
    pub unsafe fn edit<R>(
        &self,
        cx: &mut NativeContext<'_>,
        operation: Option<&NativeOperationGuard<'_>>,
        plan: EditPlan,
        edit: impl for<'a> FnOnce(&'a mut T, &mut StorageAccess<'a>) -> NativeResult<R>,
    ) -> NativeResult<R> {
        cx.poll()?;
        let Value::GcHandle(id) = self.value(cx.runtime().gc())? else {
            unreachable!("opaque root")
        };
        cx.runtime()
            .gc()
            .edit_native(cx.runtime(), id, operation, plan, edit)
    }

    /// Protect receiver slots across callbacks, with optional sealed recovery.
    ///
    /// # Safety
    /// Recovery must not allocate, panic, grow charge, execute script or retain new
    /// edges. All work state stays traced in this payload; recovery uses only
    /// already admitted edges and leaves a valid scanner/accounting state. Release
    /// the guard outside payload loans; cleanup cannot run through a conflicting loan.
    pub unsafe fn try_enter_operation<'runtime>(
        &self,
        cx: &NativeContext<'runtime>,
        kind: NativeOperationKind,
        recovery: Option<fn(&mut T, &mut StorageAccess<'_>)>,
    ) -> NativeResult<Option<NativeOperationGuard<'runtime>>> {
        let runtime = cx.runtime();
        let heap = runtime.gc();
        let Value::GcHandle(id) = self.value(heap)? else {
            unreachable!("opaque root")
        };
        heap.check_native_payload::<T>(id, false)?;
        if kind == NativeOperationKind::ReadOnlyCallbacks && recovery.is_some() {
            return Err(RuntimeError::module_validation(
                "read-only operation cannot register mutable recovery",
            ));
        }
        if kind == NativeOperationKind::ExclusiveSlots {
            heap.prepare_native_recovery(id)?;
        }
        let Some(lease) = heap.try_enter_native_operation(id, kind)? else {
            return Ok(None);
        };
        let recovery = recovery.map(|recover| -> Box<Recovery> {
            Box::new(move |payload, access| {
                recover(
                    payload
                        .downcast_mut::<T>()
                        .expect("checked recovery payload"),
                    access,
                )
            })
        });
        Ok(Some(NativeOperationGuard {
            heap,
            id,
            runtime: Some(runtime),
            recovery,
            _root: self.root.clone(),
            _lease: lease,
        }))
    }
}

impl<T: NativeData> Opaque<T> {
    /// Safely read fixed edge-free data without exposing provider interiors.
    pub fn read_data<R>(
        &self,
        cx: &NativeContext<'_>,
        read: impl for<'a> FnOnce(&'a T) -> NativeResult<R>,
    ) -> NativeResult<R> {
        // SAFETY: NativeData statically excludes edges, interior mutation and dynamic charge.
        unsafe { self.read(cx, None, |payload, _| read(payload)) }
    }

    /// Lend checked fixed data for one non-reentrant edit. Completed scalar writes
    /// remain visible when the closure fails or unwinds. A reference cannot escape.
    ///
    /// ```compile_fail
    /// use kagari_runtime::native::{payload::Opaque, typed::NativeContext};
    /// fn escape<'a>(object: &Opaque<i32>, cx: &'a NativeContext<'_>) -> &'a mut i32 {
    ///     object.edit_data(cx, |value| Ok(value)).unwrap()
    /// }
    /// ```
    pub fn edit_data<R>(
        &self,
        cx: &NativeContext<'_>,
        edit: impl for<'payload> FnOnce(&'payload mut T) -> NativeResult<R>,
    ) -> NativeResult<R> {
        self.edit_checked(cx, edit)
    }
}

impl<T: NativeTake> Opaque<T> {
    /// Move the original Rust payload out. Success invalidates every old alias,
    /// including script aliases and pinned receivers. Failed take changes nothing.
    /// Active loans/operations reject extraction; no Clone bound or copy is used.
    pub fn take(&self, cx: &NativeContext<'_>) -> NativeResult<T> {
        cx.runtime().gc().ensure_no_native_borrow()?;
        cx.runtime().resources().ensure_execution_allowed()?;
        cx.poll()?;
        let Value::GcHandle(id) = self.value(cx.runtime().gc())? else {
            unreachable!("opaque root")
        };
        cx.runtime().gc().take_native::<T>(id)
    }
}

impl NativeContext<'_> {
    /// Prepare the callback's declared native result for a managed field builder.
    pub fn result_native_type<T: NativePayload>(&self) -> NativeResult<NativeType<T>> {
        let function = self.function.ok_or_else(|| {
            RuntimeError::module_validation("native construction requires a declared result")
        })?;
        self.runtime()
            .prepare_native_result_type::<T>(self.conversion.owner(), function)
    }
}

impl<T> Opaque<T> {
    fn value(&self, heap: &GcHeap) -> NativeResult<Value> {
        self.root
            .value(heap)
            .filter(|value| heap.validate_value(value))
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
