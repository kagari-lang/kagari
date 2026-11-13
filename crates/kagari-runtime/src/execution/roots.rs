//! One complete source; stable coordinates survive growth and execution reuse.
mod cleanup;
#[cfg(feature = "execution-diagnostics")]
mod diagnostics;
pub(crate) mod frames;
pub(crate) mod pending;
pub(crate) mod queued;
pub(crate) mod returns;
mod storage;
use crate::{
    execution::space::{self, ExecutionSpace},
    value::{
        carriers::{ScopedValue, StoredValue},
        view::ValueView,
    },
};
use kagari_gc::{
    access::Scope,
    error::GcError,
    object::{AnyObject, Edge, Gc, GcObject},
    root_source::{RootAccess, RootSource},
    scan::{EdgeVisitor, ScanBudget, ScanStatus},
};
use std::{
    array, fmt,
    marker::PhantomData,
    mem,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

/// Preserve the bank's ordinary growth policy; opt-in diagnostics observe only
/// actual capacity changes. The non-diagnostic build has no counters or snapshots.
fn reserve_bank<T>(
    values: &mut Vec<T>,
    additional: usize,
    space: &ExecutionSpace,
) -> Result<(), GcError> {
    #[cfg(feature = "execution-diagnostics")]
    let before = values.capacity();
    space::reserve(values, additional, space)?;
    #[cfg(feature = "execution-diagnostics")]
    diagnostics::reservation(before, values);
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct SessionId {
    owner: u64,
    index: usize,
    generation: u64,
}

impl SessionId {
    /// A directory coordinate, never admission authority on its own. Consumers
    /// compare the complete identity before reading a control record at it.
    pub(crate) fn index(self) -> usize {
        self.index
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ValueBank {
    Managed,
    Local,
    Queued,
    Pending,
}

impl ValueBank {
    fn index(self) -> usize {
        match self {
            Self::Managed => 0,
            Self::Local => 1,
            Self::Queued => 2,
            Self::Pending => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ValueLocation {
    pub execution: SessionId,
    pub bank: ValueBank,
    pub slot: usize,
}

/// A non-owning typed coordinate in the current frame/Native dependency suffix.
/// Only that suffix's owner may keep this key; managed objects store edges instead.
pub(crate) struct DependencyId<T> {
    execution: SessionId,
    slot: usize,
    marker: PhantomData<fn() -> T>,
}

impl<T> fmt::Debug for DependencyId<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DependencyId")
            .field("execution", &self.execution)
            .field("slot", &self.slot)
            .finish()
    }
}

impl<T> Copy for DependencyId<T> {}

impl<T> Clone for DependencyId<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> DependencyId<T> {
    pub(crate) fn execution(self) -> SessionId {
        self.execution
    }
}

/// Linear ownership of one conversion-local suffix. It contains no heap address.
pub(crate) struct LocalMark {
    execution: SessionId,
    top: usize,
    return_top: usize,
    dependency_top: usize,
    frame_depth: usize,
    native_depth: usize,
}

impl LocalMark {
    pub(crate) fn execution(&self) -> SessionId {
        self.execution
    }

    pub(crate) fn top(&self) -> usize {
        self.top
    }
}

/// One synchronous Native suffix. The result names protected storage, never an
/// unrooted heap identity. Argument/local/dependency banks stay in this source.
struct NativeRecord {
    result: ValueLocation,
    origin: Option<usize>,
}

/// Linear ownership of a Native local/logical suffix. It contains no heap
/// address and is consumed by retirement rather than reused after slot recycling.
pub(crate) struct NativeSourceMark {
    execution: SessionId,
    local_top: usize,
    dependency_top: usize,
    native_top: usize,
    frame_depth: usize,
    return_top: usize,
}

impl NativeSourceMark {
    pub(crate) fn result_location(&self) -> ValueLocation {
        ValueLocation {
            execution: self.execution,
            bank: ValueBank::Local,
            slot: self.local_top,
        }
    }
}

#[derive(Default)]
pub(crate) struct ExecutionStorage {
    space: Option<Arc<ExecutionSpace>>,
    banks: [Vec<StoredValue>; 4],
    pub scalars: Vec<u64>,
    pub initialized: Vec<bool>,
    native_calls: Vec<NativeRecord>,
    dependencies: Vec<Edge<AnyObject>>,
    frame_depth: usize,
    returns: Vec<returns::ReturnRecord>,
    return_sequence: u64,
    queue_sequence: u64,
    queue: Option<queued::QueueRecord>,
}

impl ExecutionStorage {
    pub fn backing_bytes(&self) -> usize {
        self.banks
            .iter()
            .map(|bank| bank.capacity() * size_of::<StoredValue>())
            .sum::<usize>()
            + self.scalars.capacity() * size_of::<u64>()
            + self.initialized.capacity() * size_of::<bool>()
            + self.native_calls.capacity() * size_of::<NativeRecord>()
            + self.dependencies.capacity() * size_of::<Edge<AnyObject>>()
            + self.returns.capacity() * size_of::<returns::ReturnRecord>()
    }
}

struct Entry {
    generation: u64,
    storage: Option<ExecutionStorage>,
}

pub(crate) struct ExecutionRoots {
    owner: u64,
    entries: Vec<Entry>,
    // One most-recent retired execution, always edge-free and inaccessible by ID.
    reusable: ExecutionStorage,
}

pub(crate) struct Cursor {
    end: usize,
    execution: usize,
    active: Option<SessionId>,
    bank: usize,
    slot: usize,
    frontier: [usize; 5],
}

pub(crate) struct Slots<'a, 'gc> {
    source: &'a mut ExecutionRoots,
    cx: &'a mut Scope<'gc>,
}

impl ExecutionRoots {
    pub(crate) fn inspect_space(&self, id: SessionId) -> Result<Arc<ExecutionSpace>, GcError> {
        Ok(self
            .inspect_storage(id)?
            .space
            .as_ref()
            .expect("admitted execution space")
            .clone())
    }

    fn inspect_storage(&self, id: SessionId) -> Result<&ExecutionStorage, GcError> {
        if self.owner != id.owner {
            return Err(GcError::ForeignHeap);
        }
        self.entries
            .get(id.index)
            .filter(|entry| entry.generation == id.generation)
            .and_then(|entry| entry.storage.as_ref())
            .ok_or(GcError::Index)
    }

    pub(crate) fn inspect_native_origin<T>(
        &self,
        execution: SessionId,
    ) -> Result<Option<DependencyId<T>>, GcError> {
        let storage = self.inspect_storage(execution)?;
        Ok(storage
            .native_calls
            .last()
            .and_then(|native| native.origin)
            .map(|slot| DependencyId {
                execution,
                slot,
                marker: PhantomData,
            }))
    }

    /// Cold source queries return coordinates/scalars, never an edge or payload
    /// loan. The frame/diagnostic owner bounds any managed view's lifetime.
    pub(crate) fn inspect_value<'view>(
        &self,
        location: ValueLocation,
    ) -> Result<ValueView<'view>, GcError> {
        self.inspect_storage(location.execution)?.banks[location.bank.index()]
            .get(location.slot)
            .ok_or(GcError::Index)?
            .source_view(location)
    }

    pub(crate) fn inspect_scalar(
        &self,
        execution: SessionId,
        slot: usize,
    ) -> Result<Option<u64>, GcError> {
        let storage = self.inspect_storage(execution)?;
        let bits = *storage.scalars.get(slot).ok_or(GcError::Index)?;
        let initialized = *storage.initialized.get(slot).ok_or(GcError::Index)?;
        Ok(initialized.then_some(bits))
    }

    pub fn with<'gc, T>(
        roots: &mut RootAccess<'_, Self>,
        cx: &mut Scope<'gc>,
        operation: impl FnOnce(&mut Slots<'_, 'gc>) -> Result<T, GcError>,
    ) -> Result<T, GcError> {
        // SAFETY: the paired source remains in its driver. Only this checked
        // facade is lent; publication precedes every managed commit.
        let source = unsafe { roots.source_mut(cx)? };
        operation(&mut Slots { source, cx })
    }
}

impl<'gc> Slots<'_, 'gc> {
    /// The Native owner admitted original inputs in this same paired scope.
    /// Reserve before publication, then own the pending cell and the whole local/
    /// dependency suffix with one mark. Nested calls always get distinct cells.
    pub(crate) fn enter_native(
        &mut self,
        execution: SessionId,
    ) -> Result<NativeSourceMark, GcError> {
        let storage = self.storage(execution)?;
        space::reserve(
            &mut storage.native_calls,
            1,
            storage.space.as_deref().expect("admitted execution space"),
        )?;
        reserve_bank(
            &mut storage.banks[ValueBank::Local.index()],
            1,
            storage.space.as_deref().expect("admitted execution space"),
        )?;
        let mark = NativeSourceMark {
            execution,
            local_top: storage.banks[ValueBank::Local.index()].len(),
            dependency_top: storage.dependencies.len(),
            native_top: storage.native_calls.len(),
            frame_depth: storage.frame_depth,
            return_top: storage.returns.len(),
        };
        // Unit has no edge to publish. Both pushes have reserved capacity and
        // execute before the paired scope can end or any callback can run.
        storage.banks[ValueBank::Local.index()].push(StoredValue::unit());
        #[cfg(feature = "execution-diagnostics")]
        diagnostics::initialized::<StoredValue>(1);
        storage.native_calls.push(NativeRecord {
            result: mark.result_location(),
            origin: None,
        });
        Ok(mark)
    }

    /// Bind the already admitted body once. The record stores only a coordinate;
    /// the actual edge remains in this execution's dependency bank/prefix.
    pub(crate) fn set_native_origin<T: GcObject>(
        &mut self,
        mark: &NativeSourceMark,
        body: DependencyId<T>,
    ) -> Result<(), GcError> {
        self.native_result(mark)?;
        if body.execution != mark.execution {
            return Err(GcError::Storage(
                "Native origin belongs to another execution",
            ));
        }
        self.dependency(body)?;
        let record = self
            .storage(mark.execution)?
            .native_calls
            .last_mut()
            .expect("admitted current Native mark");
        if record.origin.is_some() {
            return Err(GcError::Storage("Native origin already admitted"));
        }
        record.origin = Some(body.slot);
        Ok(())
    }

    pub(crate) fn native_depth(&mut self, execution: SessionId) -> Result<usize, GcError> {
        Ok(self.storage(execution)?.native_calls.len())
    }

    pub(crate) fn native_result(
        &mut self,
        mark: &NativeSourceMark,
    ) -> Result<ValueLocation, GcError> {
        let storage = self.storage(mark.execution)?;
        if storage.native_calls.len() != mark.native_top + 1
            || storage.frame_depth != mark.frame_depth
        {
            return Err(GcError::Storage("Native suffix is not current"));
        }
        Ok(storage.native_calls[mark.native_top].result)
    }

    pub(crate) fn retire_native(&mut self, mark: NativeSourceMark) -> Result<(), GcError> {
        self.native_result(&mark)?;
        let storage = self.storage(mark.execution)?;
        let locals = &mut storage.banks[ValueBank::Local.index()];
        if mark.local_top > locals.len()
            || mark.dependency_top > storage.dependencies.len()
            || mark.return_top > storage.returns.len()
        {
            return Err(GcError::Storage("Native storage suffix was removed"));
        }
        locals.truncate(mark.local_top);
        storage.dependencies.truncate(mark.dependency_top);
        storage.native_calls.truncate(mark.native_top);
        storage.returns.truncate(mark.return_top);
        Ok(())
    }

    pub fn enter(&mut self, slots: usize, limit: usize) -> Result<SessionId, GcError> {
        let index = self
            .source
            .entries
            .iter()
            .position(|entry| entry.storage.is_none() && entry.generation != u64::MAX);
        if index.is_none() {
            self.source.entries.try_reserve(1)?;
        }
        let mut storage = mem::take(&mut self.source.reusable);
        if let Err(error) = storage.prepare(slots, limit) {
            storage.clear();
            self.source.reusable = storage;
            return Err(error);
        }
        let index = match index {
            Some(index) => {
                self.source.entries[index].generation += 1;
                self.source.entries[index].storage = Some(storage);
                index
            }
            None => {
                self.source.entries.push(Entry {
                    generation: 1,
                    storage: Some(storage),
                });
                self.source.entries.len() - 1
            }
        };
        Ok(SessionId {
            owner: self.source.owner,
            index,
            generation: self.source.entries[index].generation,
        })
    }

    pub fn storage(&mut self, id: SessionId) -> Result<&mut ExecutionStorage, GcError> {
        if self.source.owner != id.owner {
            return Err(GcError::ForeignHeap);
        }
        self.source
            .entries
            .get_mut(id.index)
            .filter(|entry| entry.generation == id.generation)
            .and_then(|entry| entry.storage.as_mut())
            .ok_or(GcError::Index)
    }

    pub fn leave(&mut self, id: SessionId) -> Result<(), GcError> {
        if !self.storage(id)?.native_calls.is_empty() {
            return Err(GcError::Storage("execution has an active Native suffix"));
        }
        let mut storage = self.source.entries[id.index]
            .storage
            .take()
            .expect("execution identity admitted above");
        storage.clear();
        self.source.reusable = storage;
        Ok(())
    }

    pub(crate) fn mark_locals(&mut self, execution: SessionId) -> Result<LocalMark, GcError> {
        let storage = self.storage(execution)?;
        Ok(LocalMark {
            execution,
            top: storage.banks[ValueBank::Local.index()].len(),
            return_top: storage.returns.len(),
            dependency_top: storage.dependencies.len(),
            frame_depth: storage.frame_depth,
            native_depth: storage.native_calls.len(),
        })
    }

    pub(crate) fn reserve_locals(
        &mut self,
        execution: SessionId,
        count: usize,
    ) -> Result<(), GcError> {
        let storage = self.storage(execution)?;
        reserve_bank(
            &mut storage.banks[ValueBank::Local.index()],
            count,
            storage.space.as_deref().expect("admitted execution space"),
        )?;
        Ok(())
    }

    pub(crate) fn push_local(
        &mut self,
        execution: SessionId,
        value: ScopedValue<'gc>,
    ) -> Result<ValueLocation, GcError> {
        let storage = self.storage(execution)?;
        let values = &mut storage.banks[ValueBank::Local.index()];
        reserve_bank(
            values,
            1,
            storage.space.as_deref().expect("admitted execution space"),
        )?;
        let slot = values.len();
        // SAFETY: the paired source commits this reserved initialized cell before
        // the scope ends; a failed publication leaves the source length unchanged.
        let stored = unsafe { StoredValue::publish_source(self.cx, value)? };
        self.storage(execution)?.banks[ValueBank::Local.index()].push(stored);
        #[cfg(feature = "execution-diagnostics")]
        diagnostics::initialized::<StoredValue>(1);
        Ok(ValueLocation {
            execution,
            bank: ValueBank::Local,
            slot,
        })
    }

    /// Publish a completed value before releasing its provider/source loan.
    /// The enclosing execution owner bounds the returned view's lifetime.
    pub(crate) fn protect_local<'view>(
        &mut self,
        execution: SessionId,
        value: ScopedValue<'gc>,
    ) -> Result<ValueView<'view>, GcError> {
        let location = value
            .object()
            .map(|_| self.push_local(execution, value))
            .transpose()?;
        ValueView::protected(value, location)
    }

    /// Publish a child result into its parent's retained prefix before retiring
    /// child scratch. The linear mark then excludes that one result from cleanup.
    pub(crate) fn preserve_local(
        &mut self,
        mark: &mut LocalMark,
        value: ScopedValue<'gc>,
    ) -> Result<ValueLocation, GcError> {
        let location = ValueLocation {
            execution: mark.execution,
            bank: ValueBank::Local,
            slot: mark.top,
        };
        let storage = self.storage(mark.execution)?;
        if mark.return_top > storage.returns.len() {
            return Err(GcError::Storage("conversion return suffix was removed"));
        }
        let len = storage.banks[ValueBank::Local.index()].len();
        if mark.top > len {
            return Err(GcError::Storage("conversion local suffix was removed"));
        }
        if mark.top == len {
            self.grow(
                mark.execution,
                ValueBank::Local,
                len.checked_add(1).ok_or(GcError::SourceLimit)?,
            )?;
        }
        self.set(location, value)?;
        // The preserved value replaces preparation scratch. Opaque returns made
        // inside that scratch must expire before its coordinates can be reused.
        self.storage(mark.execution)?
            .returns
            .truncate(mark.return_top);
        mark.top += 1;
        Ok(location)
    }

    /// Keep a completed argument pack and discard the preparation suffix before
    /// it. Edges were published while appending; relocation cannot expose a new
    /// target or leave an unprotected gap, including an already scanned prefix.
    pub(crate) fn preserve_local_pack(
        &mut self,
        mark: &mut LocalMark,
        start: usize,
        count: usize,
    ) -> Result<(), GcError> {
        let storage = self.storage(mark.execution)?;
        if mark.return_top > storage.returns.len() {
            return Err(GcError::Storage("conversion return suffix was removed"));
        }
        let values = &mut storage.banks[ValueBank::Local.index()];
        if start < mark.top || start.checked_add(count) != Some(values.len()) {
            return Err(GcError::Storage("argument pack is not the current suffix"));
        }
        // Stored carriers move as owned values. Removed preparation cells are
        // dropped without dereferencing targets; no callback or collection runs.
        values.drain(mark.top..start);
        storage.returns.truncate(mark.return_top);
        mark.top += count;
        Ok(())
    }

    /// Preserve one authenticated metadata target in the enclosing preparation
    /// owner before the child conversion retires its remaining dependency suffix.
    pub(crate) fn preserve_dependency<T: GcObject>(
        &mut self,
        mark: &mut LocalMark,
        value: Gc<'gc, T>,
    ) -> Result<DependencyId<T>, GcError> {
        let storage = self.storage(mark.execution)?;
        if storage.frame_depth != mark.frame_depth
            || storage.native_calls.len() != mark.native_depth
            || mark.dependency_top > storage.dependencies.len()
        {
            return Err(GcError::Storage(
                "metadata preservation crosses a live call",
            ));
        }
        let next = mark
            .dependency_top
            .checked_add(1)
            .ok_or(GcError::SourceLimit)?;
        let additional = usize::from(mark.dependency_top == storage.dependencies.len());
        space::reserve(
            &mut storage.dependencies,
            additional,
            storage.space.as_deref().expect("admitted execution space"),
        )?;
        // SAFETY: the preserved target is scope-protected and committed to this
        // paired source before the child suffix can retire. Publish before write.
        let edge = unsafe { self.cx.source_edge(value.erase())? };
        let storage = self.storage(mark.execution)?;
        if mark.dependency_top == storage.dependencies.len() {
            storage.dependencies.push(edge);
        } else {
            storage.dependencies[mark.dependency_top] = edge;
        }
        let source = DependencyId {
            execution: mark.execution,
            slot: mark.dependency_top,
            marker: PhantomData,
        };
        mark.dependency_top = next;
        Ok(source)
    }

    pub(crate) fn retire_locals(&mut self, mark: LocalMark) -> Result<(), GcError> {
        let storage = self.storage(mark.execution)?;
        if storage.frame_depth != mark.frame_depth
            || storage.native_calls.len() != mark.native_depth
        {
            return Err(GcError::Storage("conversion suffix crosses a live call"));
        }
        let values = &mut storage.banks[ValueBank::Local.index()];
        if mark.top > values.len()
            || mark.return_top > storage.returns.len()
            || mark.dependency_top > storage.dependencies.len()
        {
            return Err(GcError::Storage("conversion local suffix was removed"));
        }
        values.truncate(mark.top);
        storage.returns.truncate(mark.return_top);
        storage.dependencies.truncate(mark.dependency_top);
        Ok(())
    }

    /// Retain a typed internal node without assigning it a language Value tag or
    /// registering a Host root. Publication precedes the source-cell commit.
    pub(crate) fn push_dependency<T: 'static>(
        &mut self,
        execution: SessionId,
        value: Gc<'gc, T>,
    ) -> Result<DependencyId<T>, GcError> {
        let storage = self.storage(execution)?;
        space::reserve(
            &mut storage.dependencies,
            1,
            storage.space.as_deref().expect("admitted execution space"),
        )?;
        let slot = storage.dependencies.len();
        // SAFETY: this paired source owns the reserved destination. The infallible
        // push below publishes the initialized carrier before this scope ends.
        let edge = unsafe { self.cx.source_edge(value.erase())? };
        self.storage(execution)?.dependencies.push(edge);
        Ok(DependencyId {
            execution,
            slot,
            marker: PhantomData,
        })
    }

    pub(crate) fn dependency<T: GcObject>(
        &mut self,
        dependency: DependencyId<T>,
    ) -> Result<Gc<'gc, T>, GcError> {
        let value = self.dependency_at(dependency.execution, dependency.slot)?;
        self.cx.downcast(value)
    }

    pub(crate) fn dependency_count(&mut self, execution: SessionId) -> Result<usize, GcError> {
        Ok(self.storage(execution)?.dependencies.len())
    }

    /// Inspect one cell in an explicitly owned source, never a semantic-ID table.
    pub(crate) fn dependency_at(
        &mut self,
        execution: SessionId,
        index: usize,
    ) -> Result<Gc<'gc, AnyObject>, GcError> {
        self.storage(execution)?;
        let edge = self.source.entries[execution.index]
            .storage
            .as_ref()
            .and_then(|storage| storage.dependencies.get(index))
            .ok_or(GcError::Index)?;
        // SAFETY: this authenticated initialized cell belongs to the paired
        // source. The returned brand cannot escape the collection-free interval.
        unsafe { self.cx.source_get(edge) }
    }

    pub fn grow(&mut self, id: SessionId, bank: ValueBank, count: usize) -> Result<(), GcError> {
        let storage = self.storage(id)?;
        let values = &mut storage.banks[bank.index()];
        let additional = count.saturating_sub(values.len());
        reserve_bank(
            values,
            additional,
            storage.space.as_deref().expect("admitted execution space"),
        )?;
        if count > values.len() {
            values.resize_with(count, StoredValue::unit);
            #[cfg(feature = "execution-diagnostics")]
            diagnostics::initialized::<StoredValue>(additional);
        }
        Ok(())
    }

    pub fn len(&mut self, id: SessionId, bank: ValueBank) -> Result<usize, GcError> {
        Ok(self.storage(id)?.banks[bank.index()].len())
    }

    pub fn get(&mut self, location: ValueLocation) -> Result<ScopedValue<'gc>, GcError> {
        self.storage(location.execution)?;
        let value = self.source.entries[location.execution.index]
            .storage
            .as_ref()
            .and_then(|storage| storage.banks[location.bank.index()].get(location.slot))
            .ok_or(GcError::Index)?;
        // SAFETY: this checked location is an initialized, still-retained cell
        // in the same paired source. The scoped import copies no stored carrier.
        unsafe { value.get_source(self.cx) }
    }

    pub fn set(&mut self, location: ValueLocation, value: ScopedValue<'gc>) -> Result<(), GcError> {
        self.storage(location.execution)?.banks[location.bank.index()]
            .get(location.slot)
            .ok_or(GcError::Index)?;
        // SAFETY: the destination was admitted above. Publication precedes its
        // infallible coupled commit, and the returned carrier cannot escape.
        let stored = unsafe { StoredValue::publish_source(self.cx, value)? };
        self.storage(location.execution)?.banks[location.bank.index()][location.slot] = stored;
        Ok(())
    }

    #[cfg(test)]
    pub fn copy(
        &mut self,
        destination: ValueLocation,
        source: ValueLocation,
    ) -> Result<(), GcError> {
        let value = self.get(source)?;
        self.set(destination, value)
    }

    pub fn scope(&mut self) -> &mut Scope<'gc> {
        self.cx
    }
}

// SAFETY: source mutation is encapsulated by Slots and publishes all incoming
// edges. Slots are born empty. Captured finite frontiers use coordinates and
// generations, never Vec addresses; changed/new suffixes publish independently.
// Every value bank and typed-node dependency is scanned. Removing a
// slot/execution drops inert carriers without dereferencing any managed target.
// Reusable backing has no initialized carriers/dependencies and is never a root.
unsafe impl RootSource for ExecutionRoots {
    type Cursor = Cursor;

    fn empty() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self {
            owner: NEXT
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                .expect("execution source identities"),
            entries: Vec::new(),
            reusable: ExecutionStorage::default(),
        }
    }

    fn begin_scan(&self) -> Result<Cursor, GcError> {
        Ok(Cursor {
            end: self.entries.len(),
            execution: 0,
            active: None,
            bank: 0,
            slot: 0,
            frontier: [0; 5],
        })
    }

    fn scan(
        &self,
        cursor: &mut Cursor,
        budget: &mut ScanBudget,
        visitor: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        while cursor.execution < cursor.end {
            if !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            #[cfg(feature = "execution-diagnostics")]
            diagnostics::examined();
            let entry = &self.entries[cursor.execution];
            let id = SessionId {
                owner: self.owner,
                index: cursor.execution,
                generation: entry.generation,
            };
            if let Some(storage) = &entry.storage
                && cursor.active.is_none_or(|active| active == id)
            {
                if cursor.active.is_none() {
                    cursor.active = Some(id);
                    cursor.frontier = array::from_fn(|bank| {
                        if bank < 4 {
                            storage.banks[bank].len()
                        } else {
                            storage.dependencies.len()
                        }
                    });
                    cursor.bank = 0;
                    cursor.slot = 0;
                }
                while cursor.bank < 5 {
                    while cursor.slot < cursor.frontier[cursor.bank] {
                        if !budget.take(1) {
                            return Ok(ScanStatus::Pending);
                        }
                        #[cfg(feature = "execution-diagnostics")]
                        diagnostics::examined();
                        let status = if cursor.bank < 4 {
                            match storage.banks[cursor.bank].get(cursor.slot) {
                                // SAFETY: retained initialized source cell. Pending
                                // retries before advancing the captured frontier.
                                Some(value) => unsafe { value.scan(budget, visitor)? },
                                None => ScanStatus::Complete,
                            }
                        } else {
                            match storage.dependencies.get(cursor.slot) {
                                // SAFETY: this source retains its initialized edge;
                                // removal leaves a charged hole, never a pointer loan.
                                Some(edge) => unsafe { visitor.try_visit_stored(edge, budget)? },
                                None => ScanStatus::Complete,
                            }
                        };
                        if status == ScanStatus::Pending {
                            return Ok(ScanStatus::Pending);
                        }
                        cursor.slot += 1;
                    }
                    cursor.bank += 1;
                    cursor.slot = 0;
                }
            }
            cursor.execution += 1;
            cursor.active = None;
        }
        Ok(ScanStatus::Complete)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kagari_gc::{heap::HeapConfig, root_source::RootDriver};

    #[test]
    fn exhausted_generations_retire_session_slots() -> Result<(), GcError> {
        let mut driver = RootDriver::<ExecutionRoots>::new(HeapConfig::default())?;
        driver.scope(|cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                slots.source.entries.push(Entry {
                    generation: u64::MAX - 1,
                    storage: None,
                });
                let last = slots.enter(0, 8 * 1024 * 1024)?;
                assert_eq!(last.index, 0);
                assert_eq!(last.generation, u64::MAX);
                slots.leave(last)?;
                let next = slots.enter(0, 8 * 1024 * 1024)?;
                assert_eq!(next.index, 1);
                assert_ne!(next, last);
                assert!(matches!(slots.storage(last), Err(GcError::Index)));
                slots.leave(next)
            })
        })
    }
}
