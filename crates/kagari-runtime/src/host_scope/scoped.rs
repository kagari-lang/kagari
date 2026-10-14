//! Branded host lending reserves Rust storage until deterministic scope exit.
use crate::{
    Runtime,
    error::RuntimeError,
    host::HostBorrowTable,
    host_scope::HostResourceScope,
    native::{binding::NativeResult, payload::NativeType, storage::NativePayload},
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
    brand: PhantomData<(&'scope T, &'scope mut &'scope (), Rc<()>)>,
}

/// A mutable external view; copies of its script identity remain dynamically
/// checked capabilities, rather than independent exclusive Rust references.
pub struct RefMut<'scope, T> {
    value: Value,
    brand: PhantomData<(&'scope mut T, &'scope mut &'scope (), Rc<()>)>,
}

impl Runtime {
    /// Lend external state under a fresh scope. The closure's output is independent
    /// of that brand. Cleanup also runs when the closure returns an error or unwinds.
    ///
    /// A view cannot escape:
    /// ```compile_fail
    /// use kagari_runtime::{Runtime, native::payload::NativeType};
    /// fn escape(runtime: &mut Runtime, ty: &NativeType<i32>, value: &i32) {
    ///     let view = runtime.with_host_scope(|scope| scope.borrow(ty, value)).unwrap();
    ///     drop(view);
    /// }
    /// ```
    /// The root remains borrowed until the scope ends, even after dropping its view:
    /// ```compile_fail
    /// use kagari_runtime::{Runtime, native::payload::NativeType};
    /// fn modify(runtime: &mut Runtime, ty: &NativeType<i32>, value: &mut i32) {
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
    /// Reserve shared external storage for this scope, using its installed native
    /// descriptor. Foreign descriptors and incompatible root aliases fail.
    pub fn borrow<T: NativePayload>(
        &self,
        ty: &NativeType<T>,
        value: &'scope T,
    ) -> NativeResult<Ref<'scope, T>> {
        // SAFETY: The signature retains a shared T loan for the invariant brand.
        // with_host_scope reserves Runtime exclusively and removes the frame on
        // every exit before this brand's host loans can end.
        let value =
            unsafe { self.register(ty, (value as *const T).cast_mut(), ReferenceAccess::Shared)? };
        Ok(Ref {
            value,
            brand: PhantomData,
        })
    }

    /// Reserve exclusive external storage until scope exit. This is a capability
    /// reservation; active Rust references are lent only by checked operations.
    pub fn borrow_mut<T: NativePayload>(
        &self,
        ty: &NativeType<T>,
        value: &'scope mut T,
    ) -> NativeResult<RefMut<'scope, T>> {
        // SAFETY: The signature reserves T exclusively for the invariant brand;
        // no &mut T is recreated until a checked dynamic loan is acquired.
        let value = unsafe { self.register(ty, value as *mut T, ReferenceAccess::Mutable)? };
        Ok(RefMut {
            value,
            brand: PhantomData,
        })
    }

    unsafe fn register<T: NativePayload>(
        &self,
        ty: &NativeType<T>,
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

    /// Execute through the backend's ordinary checked synchronous entry. Arguments
    /// are retained through invocation. Scoped values may never be returned, stored
    /// in owning values or used by suspended execution.
    pub fn execute(
        &self,
        arguments: &[Value],
        invoke: impl FnOnce(&Runtime, &[Value]) -> NativeResult<Value>,
    ) -> NativeResult<Value> {
        self.resources.retain_values(arguments)?;
        let result = invoke(self.resources.runtime(), arguments)?;
        HostBorrowTable::validate_no_escape(self.resources.runtime().gc(), &result)?;
        if result.contains_ephemeral(self.resources.runtime().gc()) {
            return Err(RuntimeError::host_borrow_escape(
                "scoped execution result cannot retain an ephemeral value",
            ));
        }
        Ok(result)
    }
}

impl<'scope, T: NativePayload> Ref<'scope, T> {
    /// Raw script identity for a synchronous scoped argument. It becomes inert at
    /// scope exit and is checked again by execution and every access boundary.
    pub fn value(&self) -> Value {
        self.value
    }

    /// Lend a checked shared reference for this closure. Compatible shared loans
    /// may nest; an active exclusive loan rejects this operation.
    pub fn read<R>(
        &self,
        scope: &HostScope<'scope, '_>,
        read: impl for<'loan> FnOnce(&'loan T) -> NativeResult<R>,
    ) -> NativeResult<R> {
        let runtime = scope.resources.runtime();
        let view = runtime.scoped_view(&self.value)?;
        runtime.host_borrows.read_scoped(runtime, view, read)
    }
}

impl<'scope, T: NativePayload> RefMut<'scope, T> {
    /// Raw script identity for a synchronous scoped argument. Alias creation does
    /// not bypass exclusive-loan conflict checks or extend the host lifetime.
    pub fn value(&self) -> Value {
        self.value
    }

    /// Read through the mutable capability under an ordinary shared loan.
    pub fn read<R>(
        &self,
        scope: &HostScope<'scope, '_>,
        read: impl for<'loan> FnOnce(&'loan T) -> NativeResult<R>,
    ) -> NativeResult<R> {
        let runtime = scope.resources.runtime();
        let view = runtime.scoped_view(&self.value)?;
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
        let view = runtime.scoped_view(&self.value)?;
        runtime.host_borrows.edit_scoped(runtime, view, edit)
    }
}
