//! Runtime-owned execution windows, independent of persistent host root leases.
use crate::{
    error::RuntimeError,
    execution_metadata::MetadataRoot,
    frame::{arguments::FrameArguments, types::TypeEnvironment},
    gc::GcHeap,
    module::{LoadedModule, execution::allocation::RegisterAllocation},
    value::Value,
};
use std::{
    ops::Range,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FrameSlots {
    owner: u64,
    index: usize,
    generation: u64,
}

#[derive(Debug)]
struct Window {
    generation: u64,
    range: Range<usize>,
    program: LoadedModule,
    environment: Option<TypeEnvironment>,
    registers: Option<Arc<RegisterAllocation>>,
}

impl Window {
    fn physical_index(&self, logical: usize) -> Option<usize> {
        let physical = match &self.registers {
            Some(registers) if logical < registers.slots.len() => registers.index(logical)?,
            Some(registers) => registers
                .count
                .checked_add(logical - registers.slots.len())?,
            None => logical,
        };
        (physical < self.range.len()).then_some(physical)
    }
}

/// The complete active/suspended stack is a GC root. Windows are released on all
/// frame exits; retained host values use the separate generational lease table.
#[derive(Debug)]
pub(crate) struct ExecutionValues {
    owner: u64,
    next_generation: u64,
    values: Vec<Value>,
    windows: Vec<Option<Window>>,
}

impl Default for ExecutionValues {
    fn default() -> Self {
        static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);
        Self {
            owner: NEXT_OWNER
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
                .expect("execution storage identity exhausted"),
            next_generation: 0,
            values: Vec::new(),
            windows: Vec::new(),
        }
    }
}

impl ExecutionValues {
    pub(crate) fn allocate(
        &mut self,
        count: usize,
        argument_offset: usize,
        arguments: &FrameArguments<'_>,
        program: LoadedModule,
        environment: Option<TypeEnvironment>,
        registers: Option<Arc<RegisterAllocation>>,
    ) -> Result<FrameSlots, RuntimeError> {
        if argument_offset
            .checked_add(arguments.len())
            .is_none_or(|end| end > count)
        {
            return Err(RuntimeError::module_validation("frame argument window"));
        }
        let source = arguments
            .window()
            .map(|(slots, operands)| {
                let window = self
                    .window(slots)
                    .ok_or_else(|| RuntimeError::module_validation("expired frame arguments"))?;
                if operands
                    .iter()
                    .any(|r| window.physical_index(r.index()).is_none())
                {
                    return Err(RuntimeError::module_validation(
                        "invalid frame argument register",
                    ));
                }
                Ok((slots, operands))
            })
            .transpose()?;
        let generation = self
            .next_generation
            .checked_add(1)
            .ok_or_else(|| RuntimeError::resource_limit("frame window generations"))?;
        self.values
            .try_reserve(count)
            .map_err(|_| RuntimeError::resource_limit("execution value stack"))?;
        self.windows
            .try_reserve(1)
            .map_err(|_| RuntimeError::resource_limit("execution frame windows"))?;
        let start = self.values.len();
        self.values.resize(start + count, Value::Unit);
        if let Some((source, registers)) = source {
            // Resolve indices after reserve/resize: growth can relocate the arena.
            // Caller and callee windows are disjoint, including recursive calls.
            for (index, register) in registers.iter().enumerate() {
                self.values[start + argument_offset + index] = self
                    .value(source, register.index())
                    .expect("checked source window")
                    .clone();
            }
        } else {
            for (index, value) in arguments.iter().enumerate() {
                self.values[start + argument_offset + index] = value.clone();
            }
        }
        self.next_generation = generation;
        let slots = FrameSlots {
            owner: self.owner,
            index: self.windows.len(),
            generation,
        };
        self.windows.push(Some(Window {
            generation,
            range: start..start + count,
            program,
            environment,
            registers,
        }));
        Ok(slots)
    }

    fn window(&self, slots: FrameSlots) -> Option<&Window> {
        if slots.owner != self.owner {
            return None;
        }
        self.windows
            .get(slots.index)?
            .as_ref()
            .filter(|window| window.generation == slots.generation)
    }

    #[cfg(test)]
    pub(crate) fn get(&self, slots: FrameSlots) -> Option<&[Value]> {
        self.values.get(self.window(slots)?.range.clone())
    }

    pub(crate) fn get_mut(&mut self, slots: FrameSlots) -> Option<&mut [Value]> {
        let range = self.window(slots)?.range.clone();
        self.values.get_mut(range)
    }

    pub(crate) fn value(&self, slots: FrameSlots, logical: usize) -> Option<&Value> {
        let window = self.window(slots)?;
        self.values
            .get(window.range.start + window.physical_index(logical)?)
    }

    fn value_mut(&mut self, slots: FrameSlots, logical: usize) -> Option<&mut Value> {
        let window = self.window(slots)?;
        let index = window.range.start + window.physical_index(logical)?;
        self.values.get_mut(index)
    }

    pub(crate) fn release(&mut self, slots: FrameSlots) -> Option<()> {
        let range = self.window(slots)?.range.clone();
        self.values[range].fill(Value::Unit);
        self.windows[slots.index] = None;
        while self.windows.last().is_some_and(Option::is_none) {
            self.windows.pop();
        }
        let end = self
            .windows
            .last()
            .and_then(Option::as_ref)
            .map_or(0, |window| window.range.end);
        self.values.truncate(end);
        Some(())
    }

    pub(crate) fn active_windows(&self) -> usize {
        self.windows.iter().flatten().count()
    }

    pub(crate) fn append_values(&self, roots: &mut Vec<Value>) {
        roots.extend_from_slice(&self.values);
    }

    pub(crate) fn append_metadata(&self, roots: &mut Vec<MetadataRoot>) {
        for window in self.windows.iter().flatten() {
            roots.push(MetadataRoot::Program(window.program.clone()));
            if let Some(environment) = &window.environment {
                roots.push(MetadataRoot::Environment(environment.id));
            }
        }
    }
}

impl FrameSlots {
    pub(crate) fn belongs_to(self, heap: &GcHeap) -> bool {
        heap.resources()
            .frame_values
            .try_borrow()
            .is_ok_and(|values| self.owner == values.owner)
    }

    pub(crate) fn with_value<R>(
        self,
        heap: &GcHeap,
        index: usize,
        read: impl FnOnce(&Value) -> R,
    ) -> Option<R> {
        let values = heap.resources().frame_values.try_borrow().ok()?;
        values.value(self, index).map(read)
    }

    pub(crate) fn get(self, heap: &GcHeap, index: usize) -> Option<Value> {
        self.with_value(heap, index, Value::clone)
    }

    pub(crate) fn set(self, heap: &GcHeap, index: usize, value: Value) -> Option<()> {
        heap.ensure_execution_allowed().ok()?;
        if !heap.validate_value(&value) {
            return None;
        }
        let mut values = heap.resources().frame_values.try_borrow_mut().ok()?;
        *values.value_mut(self, index)? = value;
        Some(())
    }
}

#[cfg(test)]
mod tests;
