//! Registered representation and dispatch for Rust objects and dynamic records.

use crate::{
    allocation::page::Page,
    dynamic::DynamicShape,
    error::GcError,
    object::{AnyObject, GcObject, ObjectCharge, ObjectHeader},
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus},
    table::{Table, shrink_target, shrink_vec},
};
use std::{
    alloc::{Layout, alloc, dealloc},
    any::TypeId,
    collections::HashMap,
    fmt,
    marker::PhantomData,
    ptr::{self, NonNull},
    sync::Arc,
};

/// A heap-local registration. Holding it retains layout and dispatch, not objects.
pub struct TypedDescriptor<T: GcObject> {
    pub(crate) heap: u64,
    pub(crate) descriptor: Arc<Descriptor>,
    pub(crate) marker: PhantomData<fn() -> T>,
}

impl<T: GcObject> Clone for TypedDescriptor<T> {
    fn clone(&self) -> Self {
        Self {
            heap: self.heap,
            descriptor: self.descriptor.clone(),
            marker: PhantomData,
        }
    }
}

impl<T: GcObject> fmt::Debug for TypedDescriptor<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TypedDescriptor")
            .field("heap", &self.heap)
            .field("type", &self.descriptor.type_id())
            .finish()
    }
}

type Begin = unsafe fn(NonNull<u8>, NonNull<u8>);
type Scan = unsafe fn(
    NonNull<u8>,
    NonNull<u8>,
    &mut ScanBudget,
    &mut EdgeVisitor<'_>,
) -> Result<ScanStatus, GcError>;

#[derive(Debug)]
pub(crate) struct Descriptor {
    pub(crate) id: u32,
    representation: Representation,
    pub(crate) layout: Layout,
    pub(crate) group: Option<usize>,
    pub(crate) payload_offset: usize,
    pub(crate) state_layout: Layout,
    pub(crate) kind: ScanKind,
}

#[derive(Debug)]
enum Representation {
    Rust {
        type_id: TypeId,
        begin: Begin,
        scan: Scan,
        charge: unsafe fn(NonNull<u8>) -> ObjectCharge,
        destroy: unsafe fn(NonNull<u8>),
    },
    Dynamic(DynamicShape),
}

impl Descriptor {
    fn new<T: GcObject>(id: u32) -> Result<Self, GcError> {
        if matches!(T::SCAN_KIND, ScanKind::Bounded { max_work } if max_work > 204) {
            return Err(GcError::Provider("bounded scanner exceeds 204 units"));
        }
        let (layout, payload_offset) =
            ObjectHeader::layout::<T>().map_err(|_| GcError::Limit("object layout"))?;
        Ok(Self {
            id,
            representation: Representation::Rust {
                type_id: TypeId::of::<T>(),
                begin: begin::<T>,
                scan: scan::<T>,
                charge: charge::<T>,
                destroy: destroy::<T>,
            },
            layout,
            group: Page::group(layout, T::SCAN_KIND == ScanKind::EdgeFree),
            payload_offset,
            state_layout: Layout::new::<T::ScanState>(),
            kind: T::SCAN_KIND,
        })
    }

    fn dynamic(id: u32, shape: DynamicShape) -> Result<Self, GcError> {
        let (layout, payload_offset) = Layout::new::<ObjectHeader>()
            .extend(shape.layout)
            .map_err(|_| GcError::Limit("dynamic object layout"))?;
        Ok(Self {
            id,
            layout,
            group: Page::group(layout, shape.references.is_empty()),
            payload_offset,
            state_layout: Layout::new::<usize>(),
            kind: if shape.references.is_empty() {
                ScanKind::EdgeFree
            } else {
                ScanKind::Resumable
            },
            representation: Representation::Dynamic(shape),
        })
    }

    pub(crate) fn type_id(&self) -> Option<TypeId> {
        match self.representation {
            Representation::Rust { type_id, .. } => Some(type_id),
            Representation::Dynamic(_) => None,
        }
    }

    pub(crate) fn matches<T: 'static>(&self) -> bool {
        TypeId::of::<T>() == TypeId::of::<AnyObject>() || self.type_id() == Some(TypeId::of::<T>())
    }

    pub(crate) fn shape(&self) -> Result<&DynamicShape, GcError> {
        match &self.representation {
            Representation::Dynamic(shape) => Ok(shape),
            Representation::Rust { .. } => Err(GcError::Representation),
        }
    }

    // All erased dispatch requires live initialized storage and the registered
    // aligned workspace. Dynamic metadata is descriptor-owned, never a fake T.
    pub(crate) unsafe fn begin(&self, object: NonNull<u8>, state: NonNull<u8>) {
        unsafe {
            match &self.representation {
                Representation::Rust { begin, .. } => begin(object, state),
                Representation::Dynamic(_) => state.cast::<usize>().as_ptr().write(0),
            }
        }
    }

    pub(crate) unsafe fn scan(
        &self,
        object: NonNull<u8>,
        state: NonNull<u8>,
        budget: &mut ScanBudget,
        visitor: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        unsafe {
            match &self.representation {
                Representation::Rust { scan, .. } => scan(object, state, budget, visitor),
                Representation::Dynamic(shape) => shape.scan(
                    object,
                    &mut *state.cast::<usize>().as_ptr(),
                    budget,
                    visitor,
                ),
            }
        }
    }

    pub(crate) unsafe fn charge(&self, object: NonNull<u8>) -> ObjectCharge {
        match self.representation {
            Representation::Rust { charge, .. } => unsafe { charge(object) },
            Representation::Dynamic(_) => ObjectCharge::default(),
        }
    }

    pub(crate) unsafe fn destroy(&self, object: NonNull<u8>) {
        if let Representation::Rust { destroy, .. } = self.representation {
            unsafe { destroy(object) };
        }
    }
}

unsafe fn begin<T: GcObject>(object: NonNull<u8>, state: NonNull<u8>) {
    // SAFETY: descriptor admission proves initialized T and aligned state capacity.
    unsafe {
        state
            .cast::<T::ScanState>()
            .as_ptr()
            .write(object.cast::<T>().as_ref().begin_scan())
    };
}

unsafe fn scan<T: GcObject>(
    object: NonNull<u8>,
    state: NonNull<u8>,
    budget: &mut ScanBudget,
    visitor: &mut EdgeVisitor<'_>,
) -> Result<ScanStatus, GcError> {
    // SAFETY: the active owner pins T and this exact initialized Copy state.
    unsafe {
        object.cast::<T>().as_ref().scan(
            &mut *state.cast::<T::ScanState>().as_ptr(),
            budget,
            visitor,
        )
    }
}

unsafe fn charge<T: GcObject>(object: NonNull<u8>) -> ObjectCharge {
    // SAFETY: only initialized live T is sampled under an admitted storage loan.
    unsafe { object.cast::<T>().as_ref().charge() }
}

unsafe fn destroy<T: GcObject>(object: NonNull<u8>) {
    // SAFETY: a linear retirement token owns this initialized T exactly once.
    unsafe { object.cast::<T>().as_ptr().drop_in_place() };
}

#[derive(Debug, Default)]
pub(crate) struct Descriptors {
    slots: Table<DescriptorEntry>,
    active: Vec<u32>,
    types: HashMap<TypeId, u32>,
}

#[derive(Debug)]
struct DescriptorEntry {
    descriptor: Arc<Descriptor>,
    uses: usize,
}

impl Descriptors {
    pub(crate) fn register<T: GcObject>(&mut self) -> Result<Arc<Descriptor>, GcError> {
        if let Some(id) = self.types.get(&TypeId::of::<T>()) {
            return self.get(*id).cloned();
        }
        self.types.try_reserve(1)?;
        let id = self.reserve_id()?;
        let descriptor = Arc::new(Descriptor::new::<T>(id)?);
        self.install(descriptor.clone());
        self.types.insert(TypeId::of::<T>(), id);
        Ok(descriptor)
    }

    pub(crate) fn register_dynamic(
        &mut self,
        shape: DynamicShape,
    ) -> Result<Arc<Descriptor>, GcError> {
        let id = self.reserve_id()?;
        let descriptor = Arc::new(Descriptor::dynamic(id, shape)?);
        self.install(descriptor.clone());
        Ok(descriptor)
    }

    fn reserve_id(&mut self) -> Result<u32, GcError> {
        self.active.try_reserve(1)?;
        self.slots.prepare()
    }

    fn install(&mut self, descriptor: Arc<Descriptor>) {
        let id = descriptor.id;
        let entry = DescriptorEntry {
            descriptor,
            uses: 0,
        };
        self.active.push(id);
        self.slots.insert(id, entry);
    }

    pub(crate) fn get(&self, id: u32) -> Result<&Arc<Descriptor>, GcError> {
        self.slots
            .get(id)
            .map(|entry| &entry.descriptor)
            .ok_or(GcError::Storage("descriptor"))
    }

    pub(crate) fn retain(&mut self, id: u32) -> Result<(), GcError> {
        let entry = self
            .slots
            .get_mut(id)
            .ok_or(GcError::Storage("descriptor"))?;
        entry.uses = entry
            .uses
            .checked_add(1)
            .ok_or(GcError::Limit("descriptor uses"))?;
        Ok(())
    }

    pub(crate) fn release(&mut self, id: u32) {
        let entry = &mut self.slots[id];
        entry.uses = entry.uses.checked_sub(1).expect("descriptor use credit");
    }

    pub(crate) fn len(&self) -> usize {
        self.active.len()
    }

    pub(crate) fn retire_unused(&mut self, active_index: usize) {
        let index = self.active[active_index];
        let entry = &self.slots[index];
        if entry.uses == 0 && Arc::strong_count(&entry.descriptor) == 1 {
            if let Some(type_id) = entry.descriptor.type_id() {
                self.types.remove(&type_id);
            }
            self.slots.remove(index);
            self.active.swap_remove(active_index);
        }
    }

    pub(crate) fn trim(&mut self) -> Layout {
        self.slots.trim();
        shrink_vec(&mut self.active, self.slots.len());
        if let Some(target) = shrink_target(self.types.capacity(), self.types.len()) {
            self.types.shrink_to(target);
        }
        let mut size = 0;
        let mut align = 1;
        for &id in &self.active {
            let layout = self.slots[id].descriptor.state_layout;
            size = size.max(layout.size().max(1));
            align = align.max(layout.align());
        }
        Layout::from_size_align(size, align).expect("registered workspace layouts")
    }

    pub(crate) fn storage(&self) -> (usize, usize, usize, usize) {
        let (slots, bytes) = self.slots.storage();
        (
            slots,
            bytes,
            self.active.capacity() * size_of::<u32>(),
            self.types.capacity(),
        )
    }
}

/// One reusable aligned state allocation; growth preserves initialized Copy bytes.
#[derive(Debug)]
pub(crate) struct ScanWorkspace {
    pointer: NonNull<u8>,
    layout: Layout,
}

// SAFETY: no pointers/roots in state; ownership transfers only outside active loans.
unsafe impl Send for ScanWorkspace {}

impl Default for ScanWorkspace {
    fn default() -> Self {
        Self {
            pointer: NonNull::dangling(),
            layout: Layout::new::<()>(),
        }
    }
}

impl ScanWorkspace {
    pub(crate) fn reserve(&mut self, needed: Layout) -> Result<(), GcError> {
        if needed.size().max(1) <= self.layout.size() && needed.align() <= self.layout.align() {
            return Ok(());
        }
        let layout = Layout::from_size_align(
            needed.size().max(self.layout.size()).max(1),
            needed.align().max(self.layout.align()),
        )
        .map_err(|_| GcError::Limit("scan workspace"))?;
        // SAFETY: checked nonzero layout. Preserve the old buffer on allocation failure.
        let pointer = NonNull::new(unsafe { alloc(layout) }).ok_or(GcError::Allocation)?;
        if self.layout.size() != 0 {
            // SAFETY: disjoint allocated buffers and initialized Copy state bytes.
            unsafe {
                ptr::copy_nonoverlapping(
                    self.pointer.as_ptr(),
                    pointer.as_ptr(),
                    self.layout.size(),
                );
                dealloc(self.pointer.as_ptr(), self.layout);
            }
        }
        self.pointer = pointer;
        self.layout = layout;
        Ok(())
    }

    pub(crate) fn pointer(&self) -> NonNull<u8> {
        self.pointer
    }

    pub(crate) fn bytes(&self) -> usize {
        self.layout.size()
    }

    /// Called only without an active scan; old Copy state need not be preserved.
    /// Allocation failure keeps the larger valid buffer for a later attempt.
    pub(crate) fn trim(&mut self, needed: Layout) {
        if needed.size() == 0 {
            *self = Self::default();
        } else if shrink_target(self.layout.size(), needed.size()).is_some()
            || self.layout.align() > needed.align()
        {
            let mut replacement = Self::default();
            if replacement.reserve(needed).is_ok() {
                *self = replacement;
            }
        }
    }
}

impl Drop for ScanWorkspace {
    fn drop(&mut self) {
        if self.layout.size() != 0 {
            // SAFETY: owned buffer with its original allocator layout; state is Copy.
            unsafe { dealloc(self.pointer.as_ptr(), self.layout) };
        }
    }
}
