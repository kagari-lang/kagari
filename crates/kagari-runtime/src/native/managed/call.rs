//! Collection-free access and reusable call slots without per-copy host roots.

use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    native::{
        binding::NativeResult,
        managed::{
            error::failure,
            slots::{Kind, Local, SlotRegion, Slots},
            value::{Repr, RootedValue, StoredValue, Value},
        },
    },
};
use kagari_gc::{
    access::Scope,
    error::GcError,
    heap::{CollectionReport, HeapConfig, HeapStats, StepReport},
    root_source::RootDriver,
    scan::WorkBudget,
};
use std::{
    cell::{RefCell, RefMut},
    marker::PhantomData,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    sync::Arc,
};

/// Memory integration component; this owns no execution, module or host policy.
pub struct Memory {
    driver: RefCell<RootDriver<Slots>>,
    owner: Arc<()>,
}

/// One nested Native invocation. Locals cannot escape its generative lifetime.
pub struct NativeCall<'call> {
    memory: &'call Memory,
    owner: &'call Arc<()>,
    base: usize,
}

/// A short access interval; no script-call or collection operation is exposed.
pub struct CallRegion<'a, 'gc> {
    slots: SlotRegion<'a, 'gc>,
}

impl Memory {
    fn borrow_driver(&self) -> NativeResult<RefMut<'_, RootDriver<Slots>>> {
        self.driver.try_borrow_mut().map_err(|_| {
            RuntimeError::new(
                RuntimeErrorKind::HostBorrowConflict,
                "memory access region is active",
            )
        })
    }

    pub fn new(config: HeapConfig) -> NativeResult<Self> {
        let mut driver = RootDriver::<Slots>::new(config).map_err(failure)?;
        let owner = driver
            .scope(|scope, roots| Ok(Slots::region(roots, scope)?.slots.owner.clone()))
            .map_err(failure)?;
        Ok(Self {
            driver: RefCell::new(driver),
            owner,
        })
    }

    /// Enter a call and release its working slots on success, error or unwind.
    /// Nested calls preserve the complete outer root source during collection.
    ///
    /// ```compile_fail
    /// use kagari_runtime::native::managed::call::Memory;
    /// let memory = Memory::new(Default::default()).unwrap();
    /// let escaped = memory.call(|call| call.local::<u64>()).unwrap();
    /// ```
    pub fn call<R>(
        &self,
        operation: impl for<'call> FnOnce(&mut NativeCall<'call>) -> NativeResult<R>,
    ) -> NativeResult<R> {
        let base = self
            .borrow_driver()?
            .scope(|scope, roots| Ok(Slots::region(roots, scope)?.slots.values.len()))
            .map_err(failure)?;
        let owner = self.owner.clone();
        let result = catch_unwind(AssertUnwindSafe(|| {
            operation(&mut NativeCall {
                memory: self,
                owner: &owner,
                base,
            })
        }));
        let cleanup = self.borrow_driver()?.scope(|scope, roots| {
            Slots::region(roots, scope)?.slots.values.truncate(base);
            Ok(())
        });
        match result {
            Ok(result) => {
                cleanup.map_err(failure)?;
                result
            }
            Err(panic) => resume_unwind(panic),
        }
    }

    pub fn step(&self, budget: WorkBudget) -> NativeResult<StepReport> {
        self.borrow_driver()?.step(budget).map_err(failure)
    }

    pub fn stats(&self) -> NativeResult<HeapStats> {
        Ok(self
            .driver
            .try_borrow()
            .map_err(|_| {
                RuntimeError::new(
                    RuntimeErrorKind::HostBorrowConflict,
                    "memory access region is active",
                )
            })?
            .stats())
    }

    pub fn collect(&self) -> NativeResult<CollectionReport> {
        self.borrow_driver()?.collect().map_err(failure)
    }
}

impl<'call> NativeCall<'call> {
    pub fn local<T: Kind>(&mut self) -> NativeResult<Local<'call, T>> {
        let index = self.region(|region| {
            let values = &mut region.slots.slots.values;
            values
                .try_reserve(1)
                .map_err(GcError::from)
                .map_err(failure)?;
            let index = values.len();
            values.push(StoredValue { repr: Repr::Empty });
            Ok(index)
        })?;
        Ok(Local {
            owner: self.owner,
            index,
            marker: PhantomData,
        })
    }

    /// Invoke an operation inside a fresh region. Only owned outputs can escape.
    /// Scoped-return adapters publish into a Local inside this closure.
    ///
    /// ```compile_fail
    /// use kagari_runtime::native::managed::{call::Memory, payload::NativeObject};
    /// let memory = Memory::new(Default::default()).unwrap();
    /// memory.call(|call| call.region(|r| NativeObject::alloc(r, 42u64))).unwrap();
    /// ```
    pub fn region<R>(
        &mut self,
        operation: impl for<'gc> FnOnce(&mut CallRegion<'_, 'gc>) -> NativeResult<R>,
    ) -> NativeResult<R> {
        self.memory
            .borrow_driver()?
            .scope(|scope, roots| {
                Ok(operation(&mut CallRegion {
                    slots: Slots::region(roots, scope)?,
                }))
            })
            .map_err(failure)?
    }

    pub fn collect(&mut self) -> NativeResult<CollectionReport> {
        self.memory.collect()
    }

    /// Reenter through the same driver with all outer slots still visible.
    pub fn reenter<R>(
        &mut self,
        operation: impl for<'inner> FnOnce(&mut NativeCall<'inner>) -> NativeResult<R>,
    ) -> NativeResult<R> {
        self.memory.call(operation)
    }

    pub fn copy_into<T: Kind>(
        &mut self,
        source: Local<'call, T>,
        destination: Local<'call, T>,
    ) -> NativeResult<()> {
        self.region(|r| {
            let value = r.read(source)?;
            r.write(destination, value)
        })
    }

    pub fn read_scalar(&mut self, local: Local<'call, u64>) -> NativeResult<u64> {
        self.region(|r| r.read(local))
    }

    /// Transfer scalar arguments before calling arbitrary Rust/script integration.
    /// No region or payload loan crosses the callback; destination aliasing is valid.
    pub fn invoke_into(
        &mut self,
        entry: &mut impl FnMut(&mut Self, u64, u64) -> NativeResult<u64>,
        args: (Local<'call, u64>, Local<'call, u64>),
        destination: Local<'call, u64>,
    ) -> NativeResult<()> {
        let (a, b) = self.region(|r| Ok((r.read(args.0)?, r.read(args.1)?)))?;
        let result = entry(self, a, b)?;
        self.region(|r| r.write(destination, result))
    }

    pub fn stats(&self) -> NativeResult<HeapStats> {
        self.memory.stats()
    }

    pub fn step(&mut self, budget: WorkBudget) -> NativeResult<StepReport> {
        self.memory.step(budget)
    }

    pub fn working_slots(&mut self) -> NativeResult<usize> {
        let base = self.base;
        self.region(|r| Ok(r.slots.slots.values.len() - base))
    }
}

impl<'gc> CallRegion<'_, 'gc> {
    pub fn read<T: Kind>(&mut self, local: Local<'_, T>) -> NativeResult<T::View<'gc>> {
        self.slots.read(local).map_err(failure)
    }

    pub fn write<T: Kind>(&mut self, local: Local<'_, T>, value: T::View<'gc>) -> NativeResult<()> {
        self.slots.write(local, value).map_err(failure)
    }

    /// Retain a typed snapshot for a host; ordinary per-element work uses Locals.
    pub fn root<T: Kind>(&mut self, local: Local<'_, T>) -> NativeResult<RootedValue<T>> {
        let scoped = self.read(local)?;
        T::validate(self.scope(), scoped).map_err(failure)?;
        let value = T::lower(scoped);
        let repr = match value.repr {
            Repr::Empty => return Err(failure(GcError::Storage("uninitialized host result"))),
            Repr::Absent => Repr::Absent,
            Repr::Unit => Repr::Unit,
            Repr::U64(value) => Repr::U64(value),
            Repr::Object(value) => Repr::Object(self.scope_mut().retain(value).map_err(failure)?),
        };
        Ok(RootedValue {
            owner: self.slots.slots.owner.clone(),
            repr,
            marker: PhantomData,
        })
    }

    /// Admit an immutable snapshot, rejecting foreign scalar and object owners.
    pub fn admit_value<T: Kind>(&mut self, root: &RootedValue<T>) -> NativeResult<T::View<'gc>> {
        if !Arc::ptr_eq(&root.owner, &self.slots.slots.owner) {
            return Err(failure(GcError::ForeignHeap));
        }
        let repr = match &root.repr {
            Repr::Empty => return Err(failure(GcError::Storage("uninitialized host snapshot"))),
            Repr::Absent => Repr::Absent,
            Repr::Unit => Repr::Unit,
            Repr::U64(value) => Repr::U64(*value),
            Repr::Object(value) => Repr::Object(self.scope_mut().get(value).map_err(failure)?),
        };
        T::admit(self.scope(), Value { repr }).map_err(failure)
    }

    pub fn scope(&self) -> &Scope<'gc> {
        self.slots.scope
    }

    pub fn scope_mut(&mut self) -> &mut Scope<'gc> {
        self.slots.scope
    }
}
