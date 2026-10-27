//! Runtime-local representation IDs. Live and retiring slots hold admission counts.
use crate::{
    error::RuntimeError,
    gc::directory::PayloadKind,
    module::layout_storage::StructStorageLayout,
    native::{binding::NativeResult, storage::NativeObject},
};
use std::{
    alloc::Layout,
    any::TypeId,
    collections::HashMap,
    mem::needs_drop,
    num::NonZeroU32,
    ptr::NonNull,
    sync::{Arc, OnceLock, Weak},
};

#[derive(Debug, Clone)]
pub(in crate::gc) struct Descriptor {
    id: OnceLock<DescriptorId>,
    pub(super) kind: PayloadKind,
    pub(super) layout: Layout,
    pub(super) edges: bool,
    pub(in crate::gc) structure: Option<Arc<StructStorageLayout>>,
    pub(super) destroy: Option<unsafe fn(NonNull<u8>)>,
}

impl Descriptor {
    pub(super) fn new(
        kind: PayloadKind,
        layout: Layout,
        edges: bool,
        destroy: unsafe fn(NonNull<u8>),
    ) -> Self {
        Self {
            id: OnceLock::new(),
            kind,
            layout,
            edges,
            structure: None,
            destroy: Some(destroy),
        }
    }

    /// The existing StructStorageLayouts owner interns this token by semantic key.
    /// The physical descriptor table does not repeat semantic interning.
    pub(in crate::gc) fn structure(description: StructStorageLayout) -> NativeResult<Arc<Self>> {
        let layout = Layout::from_size_align(description.size, 1)
            .map_err(|_| RuntimeError::resource_limit("struct inline layout"))?;
        let mut descriptor = Self::new(
            PayloadKind::Struct,
            layout,
            !description.references.is_empty(),
            |_| {},
        );
        descriptor.structure = Some(Arc::new(description));
        // Encoded fields contain only scalar bits and non-owning heap IDs. Their
        // metadata owner lives in this descriptor, not in individual slot bytes.
        descriptor.destroy = None;
        Ok(Arc::new(descriptor))
    }

    pub(in crate::gc) fn structure_layout(&self) -> &StructStorageLayout {
        self.structure
            .as_deref()
            .expect("canonical struct representation")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::gc) struct DescriptorId(NonZeroU32);

impl DescriptorId {
    fn index(self) -> usize {
        self.0.get() as usize - 1
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Key {
    Builtin(PayloadKind),
    Native(TypeId),
}

#[derive(Debug)]
enum Entry {
    Admitted {
        descriptor: Arc<Descriptor>,
        allocations: usize,
    },
    Idle(Weak<Descriptor>),
    Free(Option<DescriptorId>),
}

#[derive(Debug, Default)]
pub(super) struct Descriptors {
    reusable: HashMap<Key, Weak<Descriptor>>,
    entries: Vec<Entry>,
    free: Option<DescriptorId>,
}

impl Descriptors {
    fn intern(
        &mut self,
        key: Key,
        make: impl FnOnce() -> Descriptor,
    ) -> NativeResult<Arc<Descriptor>> {
        if let Some(descriptor) = self.reusable.get(&key).and_then(Weak::upgrade) {
            return Ok(descriptor);
        }
        self.reusable
            .try_reserve(1)
            .map_err(|_| RuntimeError::resource_limit("page descriptors"))?;
        let descriptor = Arc::new(make());
        self.reusable.insert(key, Arc::downgrade(&descriptor));
        Ok(descriptor)
    }

    pub(super) fn builtin<T: 'static>(
        &mut self,
        kind: PayloadKind,
        edges: bool,
    ) -> NativeResult<Arc<Descriptor>> {
        self.intern(Key::Builtin(kind), || {
            let mut descriptor = Descriptor::new(kind, Layout::new::<T>(), edges, destroy::<T>);
            descriptor.destroy = needs_drop::<T>().then_some(destroy::<T>);
            descriptor
        })
    }

    pub(super) fn native(&mut self, object: &NativeObject) -> NativeResult<Arc<Descriptor>> {
        let (layout, _) = object.physical_layout()?;
        self.intern(Key::Native(object.payload_type()), || {
            Descriptor::new(PayloadKind::Native, layout, true, destroy::<NativeObject>)
        })
    }

    /// IDs cannot be recycled while any allocation, disposal token or preparation
    /// still retains this descriptor. The checked Arc identity rejects foreign tokens.
    pub(super) fn admit(&mut self, descriptor: Arc<Descriptor>) -> NativeResult<DescriptorId> {
        if let Some(&id) = descriptor.id.get() {
            match self.entries.get_mut(id.index()) {
                Some(Entry::Admitted {
                    descriptor: current,
                    allocations,
                }) if Arc::ptr_eq(current, &descriptor) => {
                    *allocations = allocations
                        .checked_add(1)
                        .ok_or_else(|| RuntimeError::resource_limit("descriptor admissions"))?;
                    return Ok(id);
                }
                Some(entry @ Entry::Idle(_)) => {
                    let Entry::Idle(weak) = entry else {
                        unreachable!()
                    };
                    if weak
                        .upgrade()
                        .is_some_and(|current| Arc::ptr_eq(&current, &descriptor))
                    {
                        *entry = Entry::Admitted {
                            descriptor,
                            allocations: 1,
                        };
                        return Ok(id);
                    }
                }
                _ => {}
            }
            return Err(RuntimeError::module_validation(
                "foreign representation token",
            ));
        }
        let id = if let Some(id) = self.free {
            id
        } else {
            let index = u32::try_from(self.entries.len())
                .ok()
                .and_then(|index| index.checked_add(1))
                .and_then(NonZeroU32::new)
                .ok_or_else(|| RuntimeError::resource_limit("descriptor identity"))?;
            self.entries
                .try_reserve(1)
                .map_err(|_| RuntimeError::resource_limit("descriptor directory"))?;
            DescriptorId(index)
        };
        descriptor.id.set(id).map_err(|_| {
            RuntimeError::module_validation("representation token admitted by another owner")
        })?;
        let entry = Entry::Admitted {
            descriptor,
            allocations: 1,
        };
        if id.index() == self.entries.len() {
            self.entries.push(entry);
        } else {
            let Entry::Free(next) = self.entries[id.index()] else {
                unreachable!("free descriptor")
            };
            self.free = next;
            self.entries[id.index()] = entry;
        }
        Ok(id)
    }

    pub(super) fn get(&self, id: DescriptorId) -> Option<&Arc<Descriptor>> {
        match self.entries.get(id.index())? {
            Entry::Admitted { descriptor, .. } => Some(descriptor),
            _ => None,
        }
    }

    pub(super) fn release(&mut self, id: DescriptorId) {
        self.release_count(id, 1);
    }

    pub(super) fn release_count(&mut self, id: DescriptorId, count: usize) {
        let entry = &mut self.entries[id.index()];
        let Entry::Admitted {
            descriptor,
            allocations,
        } = entry
        else {
            unreachable!("admitted descriptor")
        };
        *allocations = allocations
            .checked_sub(count)
            .expect("admitted descriptor count");
        if *allocations == 0 {
            *entry = Entry::Idle(Arc::downgrade(descriptor));
        }
    }

    pub(super) fn prune(&mut self) {
        self.reusable
            .retain(|_, descriptor| descriptor.strong_count() != 0);
        for (index, entry) in self.entries.iter_mut().enumerate() {
            if matches!(entry, Entry::Idle(weak) if weak.strong_count() == 0) {
                *entry = Entry::Free(self.free);
                self.free = Some(DescriptorId(NonZeroU32::new(index as u32 + 1).unwrap()));
            }
        }
    }

    #[cfg(test)]
    pub(super) fn corrupt_structure(&mut self, id: DescriptorId) {
        let Entry::Admitted { descriptor, .. } = &mut self.entries[id.index()] else {
            unreachable!()
        };
        let descriptor = Arc::make_mut(descriptor);
        Arc::make_mut(descriptor.structure.as_mut().unwrap()).fields = Box::new([]);
    }

    #[cfg(feature = "execution-diagnostics")]
    pub(super) fn usage(&self) -> (usize, usize, usize, usize, usize) {
        let records = self
            .entries
            .iter()
            .filter(|entry| !matches!(entry, Entry::Free(_)))
            .count();
        // Idle weak entries retain their Arc allocation too. Unadmitted factory
        // tokens are transient, not retained heap representation records.
        (
            records * (size_of::<Descriptor>() + 2 * size_of::<usize>()),
            records,
            self.entries.capacity() * size_of::<Entry>(),
            self.reusable.len(),
            self.reusable.capacity(),
        )
    }

    #[cfg(feature = "execution-diagnostics")]
    pub(super) fn structures(&self) -> usize {
        self.entries.iter().filter(|entry| matches!(entry, Entry::Admitted { descriptor, .. } if descriptor.structure.is_some())).count()
    }
}

unsafe fn destroy<T>(pointer: NonNull<u8>) {
    // SAFETY: sealed descriptor/layout, fully initialized representation; the token
    // clears its initialization state before this one dispatch, including unwind.
    unsafe { pointer.cast::<T>().as_ptr().drop_in_place() };
}
