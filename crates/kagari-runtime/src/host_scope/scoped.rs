//! Branded host lending reserves Rust storage until deterministic scope exit.
use crate::{
    Runtime,
    error::RuntimeError,
    gc::{
        GcHeap,
        roots::{RootSet, RootedValue},
    },
    host::HostBorrowTable,
    host_scope::{HostResourceScope, value::ScopedValue},
    native::{
        binding::NativeResult,
        rust_type::{RustField, RustType},
        scalar::NativeScalar,
        value_handle::ScriptValue,
    },
    value::{EphemeralValue, Value},
};
use kagari_types::reference::ReferenceAccess;
use std::{marker::PhantomData, rc::Rc};

/// An exclusive runtime reservation with a fresh invariant lifetime brand.
/// Execution stays inside this scope; the runtime cannot be transferred while
/// addresses are installed. Dropping it revokes views before Rust loans end.
pub struct HostScope<'scope, 'host: 'scope> {
    resources: HostResourceScope<'host>,
    brand: PhantomData<(&'scope mut &'scope (), &'host mut Runtime, Rc<()>)>,
}

/// A shared external view. It owns no Rust object and exposes no Rust reference.
pub struct Ref<'scope, T> {
    value: Value,
    heap: &'scope GcHeap,
    brand: PhantomData<(&'scope T, &'scope mut &'scope (), Rc<()>)>,
}

/// A mutable external view; copies of its script identity remain dynamically
/// checked capabilities, rather than independent exclusive Rust references.
pub struct RefMut<'scope, T> {
    value: Value,
    heap: &'scope GcHeap,
    brand: PhantomData<(&'scope mut T, &'scope mut &'scope (), Rc<()>)>,
}

impl Runtime {
    /// Lend external state under a fresh scope. The closure's output is independent
    /// of that brand. Cleanup also runs when the closure returns an error or unwinds.
    ///
    /// A view cannot escape:
    /// ```compile_fail
    /// use kagari_runtime::{Runtime, native::rust_type::RustType};
    /// fn escape(runtime: &mut Runtime, ty: &RustType<i32>, value: &i32) {
    ///     let view = runtime.with_host_scope(|scope| scope.borrow(ty, value)).unwrap();
    ///     drop(view);
    /// }
    /// ```
    /// The root remains borrowed until the scope ends, even after dropping its view:
    /// ```compile_fail
    /// use kagari_runtime::{Runtime, native::rust_type::RustType};
    /// fn modify(runtime: &mut Runtime, ty: &RustType<i32>, value: &mut i32) {
    ///     runtime.with_host_scope(|scope| {
    ///         drop(scope.borrow_mut(ty, value)?);
    ///         *value = 0;
    ///         Ok(())
    ///     }).unwrap();
    /// }
    /// ```
    /// Runtime transfer is also excluded while the scope is active:
    /// ```compile_fail
    /// use kagari_runtime::Runtime;
    /// fn transfer(mut runtime: Runtime) {
    ///     runtime.with_host_scope(|_| { drop(runtime); Ok(()) }).unwrap();
    /// }
    /// ```
    /// A lent root cannot be moved, even after consuming its view:
    /// ```compile_fail,E0505
    /// use kagari_runtime::{Runtime, native::rust_type::RustType};
    /// fn move_root(runtime: &mut Runtime, ty: &RustType<String>, value: String) {
    ///     runtime.with_host_scope(|scope| {
    ///         drop(scope.borrow(ty, &value)?);
    ///         drop(value);
    ///         Ok(())
    ///     }).unwrap();
    /// }
    /// ```
    /// A scope cannot be sent to another thread:
    /// ```compile_fail
    /// use kagari_runtime::Runtime;
    /// fn transfer(runtime: &mut Runtime) {
    ///     runtime.with_host_scope(|scope| {
    ///         std::thread::scope(|threads| { threads.spawn(move || drop(scope)); });
    ///         Ok(())
    ///     }).unwrap();
    /// }
    /// ```
    pub fn with_host_scope<'host, R>(
        &'host mut self,
        f: impl for<'scope> FnOnce(&mut HostScope<'scope, 'host>) -> NativeResult<R>,
    ) -> NativeResult<R> {
        let resources = self.host_scope(&[])?;
        f(&mut HostScope {
            resources,
            brand: PhantomData,
        })
    }
}

impl<'scope, 'host> HostScope<'scope, 'host> {
    fn project<T: Send + 'static, F: Send + 'static>(
        &self,
        argument: ScopedValue<'scope>,
        field: &RustField<T, F>,
    ) -> NativeResult<Value> {
        let runtime = self.resources.runtime();
        let value = argument.value(runtime.gc())?;
        let view = runtime.scoped_view(&value)?;
        let projected = runtime.host_borrows.project_scoped(runtime, view, field)?;
        let value = runtime
            .gc()
            .alloc_ephemeral(EphemeralValue::HostView(projected))?;
        self.resources.retain_values(&[value])?;
        runtime.gc_safepoint()?;
        Ok(value)
    }

    /// Reserve shared external storage for this scope, using its installed Rust target
    /// descriptor. Foreign descriptors and incompatible root aliases fail.
    pub fn borrow<T: Send + 'static>(
        &self,
        ty: &RustType<T>,
        value: &'scope T,
    ) -> NativeResult<Ref<'scope, T>> {
        // SAFETY: The signature retains a shared T loan for the invariant brand.
        // with_host_scope reserves Runtime exclusively and removes the frame on
        // every exit before this brand's host loans can end.
        let value =
            unsafe { self.register(ty, (value as *const T).cast_mut(), ReferenceAccess::Shared)? };
        Ok(Ref {
            value,
            heap: self.resources.runtime().gc(),
            brand: PhantomData,
        })
    }

    /// Reserve exclusive external storage until scope exit. This is a capability
    /// reservation; active Rust references are lent only by checked operations.
    pub fn borrow_mut<T: Send + 'static>(
        &self,
        ty: &RustType<T>,
        value: &'scope mut T,
    ) -> NativeResult<RefMut<'scope, T>> {
        // SAFETY: The signature reserves T exclusively for the invariant brand;
        // no &mut T is recreated until a checked dynamic loan is acquired.
        let value = unsafe { self.register(ty, value as *mut T, ReferenceAccess::Mutable)? };
        Ok(RefMut {
            value,
            heap: self.resources.runtime().gc(),
            brand: PhantomData,
        })
    }

    unsafe fn register<T: Send + 'static>(
        &self,
        ty: &RustType<T>,
        pointer: *mut T,
        access: ReferenceAccess,
    ) -> NativeResult<Value> {
        let runtime = self.resources.runtime();
        // SAFETY: Forward the host loan and exclusive runtime reservation from
        // borrow/borrow_mut; this method is private and never accepts safe raw pointers.
        let view = unsafe {
            runtime.host_borrows.register_scoped(
                runtime,
                self.resources.borrows(),
                ty,
                pointer,
                access,
            )?
        };
        let value = runtime
            .gc()
            .alloc_ephemeral(EphemeralValue::HostView(view))?;
        self.resources.retain_values(&[value])?;
        runtime.gc_safepoint()?;
        Ok(value)
    }

    /// Access the reserved runtime while this host scope is active.
    pub fn runtime(&self) -> &Runtime {
        self.resources.runtime()
    }

    /// Copy a scalar into a scoped argument without allocating a root slot.
    pub fn scalar<T: NativeScalar>(&self, value: T) -> ScopedValue<'scope> {
        ScopedValue::new(self.resources.runtime().gc(), value.encode())
    }

    /// Retain an owned result independently of its replaceable source root slot.
    /// Rejects foreign roots and values unavailable to this host scope.
    pub fn retain(&self, root: &RootedValue) -> NativeResult<ScopedValue<'scope>> {
        let heap = self.resources.runtime().gc();
        let value = root.value(heap).ok_or_else(|| {
            RuntimeError::module_validation("foreign or expired scoped argument root")
        })?;
        self.retain_value(value)
    }

    /// Retain a typed script value, including manually authored native mappings.
    /// Its exact type and runtime are checked before taking an independent snapshot.
    pub fn retain_typed(&self, value: &ScriptValue) -> NativeResult<ScopedValue<'scope>> {
        self.retain_value(value.value(self.resources.runtime())?)
    }

    /// Import one protected execution slot, preserving its original host capability.
    /// Rejects foreign packs, invalid slots and revoked external host borrows.
    pub fn retain_slot(&self, roots: &RootSet, index: usize) -> NativeResult<ScopedValue<'scope>> {
        let value = roots
            .get(self.resources.runtime().gc(), index)
            .ok_or_else(|| {
                RuntimeError::module_validation("foreign or invalid scoped argument slot")
            })?;
        self.retain_value(value)
    }

    fn retain_value(&self, value: Value) -> NativeResult<ScopedValue<'scope>> {
        self.resources.retain_values(&[value])?;
        Ok(ScopedValue::new(self.resources.runtime().gc(), value))
    }

    /// Execute a backend transport callback under this scope's argument protection.
    /// The result stays protected by this scope. Ordinary script values can be
    /// retained independently; borrowed/ephemeral results are rejected here.
    ///
    /// # Safety
    /// The backend must protect arguments through its checked frame admission and
    /// execution. Raw copies cannot escape without an independent traced owner;
    /// host capabilities additionally require their original scope. Return live
    /// transport with no intervening safepoint before this method roots it.
    #[doc(hidden)]
    pub unsafe fn execute_transport(
        &self,
        arguments: &[ScopedValue<'scope>],
        invoke: impl FnOnce(&Runtime, &[Value]) -> NativeResult<Value>,
    ) -> NativeResult<ScopedValue<'scope>> {
        let runtime = self.resources.runtime();
        let arguments = arguments
            .iter()
            .map(|value| value.value(runtime.gc()))
            .collect::<NativeResult<Vec<_>>>()?;
        self.resources.retain_values(&arguments)?;
        let result = invoke(runtime, &arguments)?;
        HostBorrowTable::validate_no_escape(runtime.gc(), &result)?;
        if result.contains_ephemeral(runtime.gc()) {
            return Err(RuntimeError::host_borrow_escape(
                "scoped execution result cannot retain an ephemeral value",
            ));
        }
        self.retain_value(result)
    }
}

impl<'scope, T: Send + 'static> Ref<'scope, T> {
    /// Prepare a child location through a generated, generation-pinned field.
    /// No Rust reference or cached address is created by this operation.
    pub fn field<F: Send + 'static>(
        &self,
        scope: &HostScope<'scope, '_>,
        field: &RustField<T, F>,
    ) -> NativeResult<Ref<'scope, F>> {
        Ok(Ref {
            value: scope.project(self.argument(), field)?,
            heap: scope.resources.runtime().gc(),
            brand: PhantomData,
        })
    }

    /// Pass this borrowed view without exposing raw managed transport.
    pub fn argument(&self) -> ScopedValue<'scope> {
        ScopedValue::new(self.heap, self.value)
    }

    /// Lend a checked shared reference for this closure. Compatible shared loans
    /// may nest; an active exclusive loan rejects this operation. A shared view
    /// cannot acquire mutable access:
    /// ```compile_fail,E0599
    /// use kagari_runtime::host_scope::scoped::{HostScope, Ref};
    /// fn upgrade<'scope>(scope: &HostScope<'scope, '_>, view: &Ref<'scope, i32>) {
    ///     view.edit(scope, |value| { *value = 0; Ok(()) }).unwrap();
    /// }
    /// ```
    pub fn read<R>(
        &self,
        scope: &HostScope<'scope, '_>,
        read: impl for<'loan> FnOnce(&'loan T) -> NativeResult<R>,
    ) -> NativeResult<R> {
        let runtime = scope.resources.runtime();
        let value = self.argument().value(runtime.gc())?;
        let view = runtime.scoped_view(&value)?;
        runtime.host_borrows.read_scoped(runtime, view, read)
    }
}

impl<'scope, T: Send + 'static> RefMut<'scope, T> {
    /// Prepare a mutable child capability; every subsequent access checks its
    /// path against active loans, including ancestors and the whole root.
    pub fn field<F: Send + 'static>(
        &self,
        scope: &HostScope<'scope, '_>,
        field: &RustField<T, F>,
    ) -> NativeResult<RefMut<'scope, F>> {
        Ok(RefMut {
            value: scope.project(self.argument(), field)?,
            heap: scope.resources.runtime().gc(),
            brand: PhantomData,
        })
    }

    /// Pass this capability within its original scope and runtime.
    /// Copying an argument does not bypass dynamic exclusive-loan checks.
    pub fn argument(&self) -> ScopedValue<'scope> {
        ScopedValue::new(self.heap, self.value)
    }

    /// Read through the mutable capability under an ordinary shared loan.
    pub fn read<R>(
        &self,
        scope: &HostScope<'scope, '_>,
        read: impl for<'loan> FnOnce(&'loan T) -> NativeResult<R>,
    ) -> NativeResult<R> {
        let runtime = scope.resources.runtime();
        let value = self.argument().value(runtime.gc())?;
        let view = runtime.scoped_view(&value)?;
        runtime.host_borrows.read_scoped(runtime, view, read)
    }

    /// Lend exclusive access without transferring ownership. Completed writes
    /// remain visible on error or unwind; the dynamic loan always releases.
    ///
    /// ```compile_fail
    /// use kagari_runtime::host_scope::scoped::{HostScope, RefMut};
    /// fn escape<'scope>(scope: &HostScope<'scope, '_>, view: &RefMut<'scope, i32>) -> &'scope mut i32 {
    ///     view.edit(scope, |value| Ok(value)).unwrap()
    /// }
    /// ```
    pub fn edit<R>(
        &self,
        scope: &HostScope<'scope, '_>,
        edit: impl for<'loan> FnOnce(&'loan mut T) -> NativeResult<R>,
    ) -> NativeResult<R> {
        let runtime = scope.resources.runtime();
        let value = self.argument().value(runtime.gc())?;
        let view = runtime.scoped_view(&value)?;
        runtime.host_borrows.edit_scoped(runtime, view, edit)
    }
}
