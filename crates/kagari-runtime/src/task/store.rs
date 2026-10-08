//! Bounded generational records. Terminal handles keep their cache in the heap.
use crate::{
    frame::{factory::QueuedFactory, types::arguments::TypeArgument},
    gc::roots::RootedValue,
    module::LoadedModule,
    resource::AsyncLimits,
    session::{ExecutionOptions, owned::OwnedExecution},
    task::{
        ScopeId, SpawnError, TaskFailure,
        control::{ScopeControl, TaskSignal},
        dependencies::Dependencies,
    },
};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Identity {
    pub owner: u64,
    pub slot: usize,
    pub generation: u64,
}

#[derive(Debug)]
struct Slot<T> {
    generation: u64,
    reserved: bool,
    value: Option<T>,
}

#[derive(Debug)]
pub(crate) struct Slots<T> {
    owner: u64,
    limit: usize,
    slots: Vec<Slot<T>>,
    free: Vec<usize>,
    count: usize,
}

impl<T> Slots<T> {
    fn new(owner: u64, limit: usize) -> Self {
        Self {
            owner,
            limit,
            slots: vec![],
            free: vec![],
            count: 0,
        }
    }

    pub fn reserve(&mut self) -> Result<Identity, SpawnError> {
        if self.count >= self.limit {
            return Err(SpawnError::CapacityExceeded);
        }
        let slot = if let Some(slot) = self.free.pop() {
            slot
        } else {
            // Retirement must not allocate while releasing a completed report.
            self.free
                .try_reserve(self.slots.len() + 1 - self.free.len())
                .map_err(|_| SpawnError::CapacityExceeded)?;
            self.slots
                .try_reserve(1)
                .map_err(|_| SpawnError::CapacityExceeded)?;
            self.slots.push(Slot {
                generation: 0,
                reserved: false,
                value: None,
            });
            self.slots.len() - 1
        };
        self.slots[slot].reserved = true;
        self.count += 1;
        Ok(Identity {
            owner: self.owner,
            slot,
            generation: self.slots[slot].generation,
        })
    }

    fn slot(&self, id: Identity) -> Option<&Slot<T>> {
        self.slots.get(id.slot).filter(|slot| {
            id.owner == self.owner && slot.generation == id.generation && slot.reserved
        })
    }

    pub fn insert(&mut self, id: Identity, value: T) {
        assert!(self.slot(id).is_some_and(|slot| slot.value.is_none()));
        self.slots[id.slot].value = Some(value);
    }

    pub fn get(&self, id: Identity) -> Option<&T> {
        self.slot(id)?.value.as_ref()
    }

    pub fn get_mut(&mut self, id: Identity) -> Option<&mut T> {
        self.slot(id)?;
        self.slots[id.slot].value.as_mut()
    }

    pub fn remove(&mut self, id: Identity) -> Option<T> {
        self.slot(id)?;
        let slot = &mut self.slots[id.slot];
        let value = slot.value.take();
        slot.reserved = false;
        self.count -= 1;
        if let Some(generation) = slot.generation.checked_add(1) {
            slot.generation = generation;
            self.free.push(id.slot);
        }
        value
    }

    pub fn iter(&self) -> impl Iterator<Item = (Identity, &T)> {
        self.slots.iter().enumerate().filter_map(|(index, slot)| {
            slot.value.as_ref().map(|value| {
                (
                    Identity {
                        owner: self.owner,
                        slot: index,
                        generation: slot.generation,
                    },
                    value,
                )
            })
        })
    }
}

#[derive(Debug)]
pub(crate) struct ScopeRecord {
    pub control: Arc<ScopeControl>,
    pub options: ExecutionOptions,
}

#[derive(Debug)]
pub(crate) enum TaskState {
    Admitting,
    Queued(QueuedFactory),
    Owned(OwnedExecution),
    Driving,
    Terminal(Result<RootedValue, TaskFailure>),
}

#[derive(Debug)]
pub(crate) struct TaskRecord {
    pub scope: ScopeId,
    pub signal: Arc<TaskSignal>,
    pub value: RootedValue,
    pub owner: LoadedModule,
    pub output: TypeArgument,
    pub options: ExecutionOptions,
    pub state: TaskState,
}

#[derive(Debug)]
pub(crate) struct TaskStore {
    pub dependencies: Arc<Dependencies>,
    pub owner: u64,
    pub scopes: Slots<ScopeRecord>,
    pub tasks: Slots<TaskRecord>,
}

impl TaskStore {
    pub fn new(limits: AsyncLimits) -> Self {
        static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);
        let owner = NEXT_OWNER
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .expect("task registry identity exhausted");
        Self {
            dependencies: Arc::new(Dependencies::new(limits.max_task_waiters)),
            owner,
            scopes: Slots::new(owner, limits.max_task_scopes.get()),
            tasks: Slots::new(owner, limits.max_tasks.get()),
        }
    }
}
