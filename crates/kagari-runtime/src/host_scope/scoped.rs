//! Branded host lending reserves Rust storage until deterministic scope exit.
use crate::{
    Runtime,
    host::{HostCallGuard, HostFrameMark},
    host_scope::HostResourceScope,
    native::{
        binding::NativeResult,
        conversion::arguments::ArgumentValues,
        rust_type::{RustField, RustType},
    },
    value::{
        EphemeralValue,
        carriers::RootedValue,
        place::{PublishedResult, ResultPlace},
        view::ValueView,
    },
};
use kagari_types::reference::ReferenceAccess;
use std::{any::TypeId, marker::PhantomData, rc::Rc};

/// An exclusive runtime reservation with a fresh invariant lifetime brand.
/// Execution stays inside this scope; the runtime cannot be transferred while
/// addresses are installed. Dropping it revokes views before Rust loans end.
pub struct HostScope<'scope, 'host: 'scope> {
    resources: HostResourceScope<'host>,
    brand: PhantomData<(&'scope mut &'scope (), &'host mut Runtime, Rc<()>)>,
}

/// Own only the dynamic loan mark while the callback reborrows the Host scope.
struct HostViewLoan<'loan, 'scope, 'host: 'scope> {
    scope: &'loan mut HostScope<'scope, 'host>,
    mark: Option<HostFrameMark>,
}

impl Drop for HostViewLoan<'_, '_, '_> {
    fn drop(&mut self) {
        if let Some(mark) = self.mark.take() {
            let runtime = self.scope.resources.runtime();
            drop(HostCallGuard::from_mark(
                &runtime.host_borrows,
                runtime.resources(),
                mark,
            ));
        }
    }
}

/// A shared external view. It owns no Rust object and exposes no Rust reference.
pub struct Ref<'scope, T> {
    value: ValueView<'scope>,
    brand: PhantomData<(&'scope T, &'scope mut &'scope (), Rc<()>)>,
}

/// A mutable external view; copies of its script identity remain dynamically
/// checked capabilities, rather than independent exclusive Rust references.
pub struct RefMut<'scope, T> {
    value: ValueView<'scope>,
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
        &mut self,
        value: ValueView<'scope>,
        field: &RustField<T, F>,
    ) -> NativeResult<ValueView<'scope>> {
        let view = self.resources.runtime_mut().scoped_view(&value)?;
        let runtime = self.resources.runtime();
        let projected = runtime.host_borrows.project_scoped(runtime, view, field)?;
        let value = self
            .resources
            .ephemeral(EphemeralValue::HostView(projected))?;
        self.resources.runtime_mut().gc_safepoint()?;
        Ok(value.reborrow())
    }

    /// Reserve shared external storage for this scope, using its installed Rust target
    /// descriptor. Foreign descriptors and incompatible root aliases fail.
    pub fn borrow<T: Send + 'static>(
        &mut self,
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
            brand: PhantomData,
        })
    }

    /// Reserve exclusive external storage until scope exit. This is a capability
    /// reservation; active Rust references are lent only by checked operations.
    pub fn borrow_mut<T: Send + 'static>(
        &mut self,
        ty: &RustType<T>,
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

    unsafe fn register<T: Send + 'static>(
        &mut self,
        ty: &RustType<T>,
        pointer: *mut T,
        access: ReferenceAccess,
    ) -> NativeResult<ValueView<'scope>> {
        let runtime = self.resources.runtime();
        // SAFETY: Forward the host loan and exclusive runtime reservation from
        // borrow/borrow_mut; this method is private and never accepts safe raw pointers.
        let view = unsafe {
            runtime.host_borrows.register_scoped(
                runtime,
                &self.resources.borrows(),
                ty,
                pointer,
                access,
            )?
        };
        let value = self.resources.ephemeral(EphemeralValue::HostView(view))?;
        self.resources.runtime_mut().gc_safepoint()?;
        Ok(value.reborrow())
    }

    /// Import genuine Host ownership into this scope without cloning its root.
    pub fn import(&mut self, value: &RootedValue) -> NativeResult<ValueView<'scope>> {
        self.resources.import(value).map(ValueView::reborrow)
    }

    /// Copy an existing context's live view into this scope's protected lifetime.
    pub fn protect(&mut self, value: &ValueView<'_>) -> NativeResult<ValueView<'scope>> {
        self.resources.protect(value).map(ValueView::reborrow)
    }

    /// Retain an ordinary result beyond the scope. Borrowed Host/Runtime values
    /// reject; this is the explicit owning escape rather than temporary transport.
    pub fn retain(&mut self, value: &ValueView<'_>) -> NativeResult<RootedValue> {
        self.resources.retain(value)
    }

    /// Invoke a backend with protected arguments and a linear result destination.
    /// The backend publishes before retiring its own scratch and returns that
    /// proof. The checked result remains local to this scope; no Host root pack
    /// bridges entry/return. Scoped Host/Runtime temporaries cannot be returned.
    pub fn execute(
        &mut self,
        arguments: &[ValueView<'_>],
        invoke: impl for<'result> FnOnce(
            &mut Runtime,
            ArgumentValues<'scope>,
            ResultPlace<'result>,
        ) -> NativeResult<PublishedResult<'result>>,
    ) -> NativeResult<ValueView<'scope>> {
        self.resources.execute(arguments, invoke)
    }
}

impl<'scope, 'host: 'scope> HostScope<'scope, 'host> {
    fn lend<T: Send + 'static, R>(
        &mut self,
        value: ValueView<'scope>,
        access: ReferenceAccess,
        operation: impl FnOnce(&mut Self, *mut T) -> NativeResult<R>,
    ) -> NativeResult<R> {
        let view = self.resources.runtime_mut().scoped_view(&value)?;
        let runtime = self.resources.runtime();
        let (mark, [pointer]) = runtime
            .host_borrows
            .prepare_scoped_loans(runtime, [(view, TypeId::of::<T>(), access)])?;
        let loan = HostViewLoan {
            scope: self,
            mark: Some(mark),
        };
        operation(&mut *loan.scope, pointer.cast())
    }
}

impl<'scope, T: Send + 'static> Ref<'scope, T> {
    /// Prepare a child location through a generated, generation-pinned field.
    /// No Rust reference or cached address is created by this operation.
    pub fn field<F: Send + 'static>(
        &self,
        scope: &mut HostScope<'scope, '_>,
        field: &RustField<T, F>,
    ) -> NativeResult<Ref<'scope, F>> {
        Ok(Ref {
            value: scope.project(self.value, field)?,
            brand: PhantomData,
        })
    }

    /// Protected view for a synchronous scoped argument. It becomes inert at
    /// scope exit and is checked again by execution and every access boundary.
    pub fn value(&self) -> ValueView<'scope> {
        self.value
    }

    /// Lend a checked shared reference for this closure. Compatible shared loans
    /// may nest; an active exclusive loan rejects this operation. The callback
    /// reborrows the same scope for checked nested execution. A shared view
    /// cannot acquire mutable access:
    /// ```compile_fail,E0599
    /// use kagari_runtime::host_scope::scoped::{HostScope, Ref};
    /// fn upgrade<'scope>(scope: &mut HostScope<'scope, '_>, view: &Ref<'scope, i32>) {
    ///     view.edit(scope, |_, value| { *value = 0; Ok(()) }).unwrap();
    /// }
    /// ```
    pub fn read<'host: 'scope, R>(
        &self,
        scope: &mut HostScope<'scope, 'host>,
        read: impl for<'loan> FnOnce(&'loan mut HostScope<'scope, 'host>, &'loan T) -> NativeResult<R>,
    ) -> NativeResult<R> {
        scope.lend(
            self.value,
            ReferenceAccess::Shared,
            |scope, pointer: *mut T| {
                // SAFETY: Exact type/projection and the complete shared loan were checked;
                // the mark stays active through reentry and the HRTB excludes escape.
                read(scope, unsafe { &*pointer })
            },
        )
    }
}

impl<'scope, T: Send + 'static> RefMut<'scope, T> {
    /// Prepare a mutable child capability; every subsequent access checks its
    /// path against active loans, including ancestors and the whole root.
    pub fn field<F: Send + 'static>(
        &self,
        scope: &mut HostScope<'scope, '_>,
        field: &RustField<T, F>,
    ) -> NativeResult<RefMut<'scope, F>> {
        Ok(RefMut {
            value: scope.project(self.value, field)?,
            brand: PhantomData,
        })
    }

    /// Protected view for a synchronous scoped argument. Alias creation does
    /// not bypass exclusive-loan conflict checks or extend the host lifetime.
    pub fn value(&self) -> ValueView<'scope> {
        self.value
    }

    /// Read through the mutable capability under an ordinary shared loan.
    pub fn read<'host: 'scope, R>(
        &self,
        scope: &mut HostScope<'scope, 'host>,
        read: impl for<'loan> FnOnce(&'loan mut HostScope<'scope, 'host>, &'loan T) -> NativeResult<R>,
    ) -> NativeResult<R> {
        scope.lend(
            self.value,
            ReferenceAccess::Shared,
            |scope, pointer: *mut T| {
                // SAFETY: Exact type/projection and the complete shared loan were checked;
                // the mark stays active through reentry and the HRTB excludes escape.
                read(scope, unsafe { &*pointer })
            },
        )
    }

    /// Lend exclusive access without transferring ownership. Completed writes
    /// remain visible on error or unwind; the dynamic loan always releases.
    /// The callback receives the same scope; nested execution still checks this loan.
    ///
    /// ```compile_fail
    /// use kagari_runtime::host_scope::scoped::{HostScope, RefMut};
    /// fn escape<'scope>(scope: &mut HostScope<'scope, '_>, view: &RefMut<'scope, i32>) -> &'scope mut i32 {
    ///     view.edit(scope, |_, value| Ok(value)).unwrap()
    /// }
    /// ```
    pub fn edit<'host: 'scope, R>(
        &self,
        scope: &mut HostScope<'scope, 'host>,
        edit: impl for<'loan> FnOnce(
            &'loan mut HostScope<'scope, 'host>,
            &'loan mut T,
        ) -> NativeResult<R>,
    ) -> NativeResult<R> {
        scope.lend(
            self.value,
            ReferenceAccess::Mutable,
            |scope, pointer: *mut T| {
                // SAFETY: The unique whole/path loan excludes incompatible aliases;
                // checked reentry retains that mark and the HRTB excludes escape.
                edit(scope, unsafe { &mut *pointer })
            },
        )
    }
}
