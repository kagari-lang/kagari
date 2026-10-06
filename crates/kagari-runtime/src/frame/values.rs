//! Runtime-owned execution windows, independent of persistent host root leases.
use kagari_abi::representation::ValueType;

pub(crate) mod scalar;

use crate::{
    error::RuntimeError,
    execution_metadata::MetadataRoot,
    frame::{arguments::FrameArguments, types::TypeEnvironment},
    gc::GcHeap,
    module::{
        LoadedModule,
        execution::{
            OperandSlot, ScalarSlot,
            layout::{FrameLayout, Location},
        },
    },
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
    ranges: WindowRanges,
    program: LoadedModule,
    environment: Option<TypeEnvironment>,
    registers: Option<Arc<FrameLayout>>,
}

#[derive(Debug, Clone)]
pub(crate) struct WindowRanges {
    pub scalars: Range<usize>,
    pub managed: Range<usize>,
}

impl Window {
    fn location(&self, logical: usize) -> Option<Location> {
        match &self.registers {
            Some(layout) => layout.location(logical),
            None if logical < self.ranges.managed.len() => Some(Location {
                operand: OperandSlot::new(logical, true),
                representation: ValueType::Generic,
                semantic: None,
            }),
            None => None,
        }
    }
}

/// The complete active/suspended stack is a GC root. Windows are released on all
/// frame exits; retained host values use the separate generational lease table.
#[derive(Debug)]
pub(crate) struct ExecutionValues {
    owner: u64,
    next_generation: u64,
    pub(crate) values: Vec<Value>,
    pub(crate) payloads: Vec<u64>,
    pub(crate) initialized: Vec<bool>,
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
            payloads: Vec::new(),
            initialized: Vec::new(),
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
        registers: Option<Arc<FrameLayout>>,
    ) -> Result<FrameSlots, RuntimeError> {
        if argument_offset
            .checked_add(arguments.len())
            .is_none_or(|end| end > count)
        {
            return Err(RuntimeError::module_validation("frame argument window"));
        }
        let (scalar_count, managed_count) = match &registers {
            Some(layout) if layout.locations.len() == count => {
                (layout.scalar_count, layout.managed_count)
            }
            Some(_) => return Err(RuntimeError::module_validation("frame layout size")),
            None => (0, count),
        };
        let destination = |index: usize| {
            registers.as_ref().map_or(
                Some(Location {
                    operand: OperandSlot::new(index, true),
                    representation: ValueType::Generic,
                    semantic: None,
                }),
                |layout| layout.location(index),
            )
        };
        // Admission is transactional: reject an invalid argument before growing
        // any bank or publishing a frame window. No partial roots can escape.
        if let Some((source, operands)) = arguments.window() {
            for (index, register) in operands.iter().enumerate() {
                let target = destination(argument_offset + index).expect("checked argument range");
                if !self.admits_transfer(source, register.index(), target, registers.is_none()) {
                    return Err(RuntimeError::module_validation(
                        "invalid frame argument type or window",
                    ));
                }
            }
        } else {
            for (index, value) in arguments.iter().enumerate() {
                if registers.is_some()
                    && !destination(argument_offset + index)
                        .expect("checked argument range")
                        .admits(value)
                {
                    return Err(RuntimeError::module_validation(
                        "invalid frame argument type or range",
                    ));
                }
            }
        }
        let generation = self
            .next_generation
            .checked_add(1)
            .ok_or_else(|| RuntimeError::resource_limit("frame window generations"))?;
        self.values
            .try_reserve(managed_count)
            .map_err(|_| RuntimeError::resource_limit("managed execution stack"))?;
        self.payloads
            .try_reserve(scalar_count)
            .map_err(|_| RuntimeError::resource_limit("scalar execution stack"))?;
        self.initialized
            .try_reserve(scalar_count)
            .map_err(|_| RuntimeError::resource_limit("execution initialization stack"))?;
        self.windows
            .try_reserve(1)
            .map_err(|_| RuntimeError::resource_limit("execution frame windows"))?;
        let scalar_start = self.payloads.len();
        let managed_start = self.values.len();
        self.payloads.resize(scalar_start + scalar_count, 0);
        self.initialized.resize(scalar_start + scalar_count, false);
        self.values
            .resize(managed_start + managed_count, Value::Unit);
        self.next_generation = generation;
        let slots = FrameSlots {
            owner: self.owner,
            index: self.windows.len(),
            generation,
        };
        self.windows.push(Some(Window {
            generation,
            ranges: WindowRanges {
                scalars: scalar_start..scalar_start + scalar_count,
                managed: managed_start..managed_start + managed_count,
            },
            program,
            environment,
            registers,
        }));
        if let Some((source, operands)) = arguments.window() {
            for (index, register) in operands.iter().enumerate() {
                // Resolve after both banks grow; caller/callee remain disjoint.
                if let Some((representation, bits)) = self.scalar_value(source, register.index())
                    && !self
                        .window(slots)
                        .expect("published window")
                        .location(argument_offset + index)
                        .expect("argument location")
                        .operand
                        .managed()
                {
                    self.set_scalar(slots, argument_offset + index, representation, bits)
                        .expect("admitted scalar argument");
                } else {
                    let value = self
                        .with_value(source, register.index(), Value::clone)
                        .expect("checked source window");
                    self.set(slots, argument_offset + index, value)
                        .expect("admitted argument");
                }
            }
        } else {
            for (index, value) in arguments.iter().enumerate() {
                self.set(slots, argument_offset + index, value.clone())
                    .expect("admitted argument");
            }
        }
        Ok(slots)
    }

    fn admits_transfer(
        &self,
        source: FrameSlots,
        logical: usize,
        target: Location,
        unrestricted: bool,
    ) -> bool {
        if let Some((representation, bits)) = self.scalar_value(source, logical)
            && !target.operand.managed()
        {
            return target.admits_payload(representation, bits);
        }
        self.with_value(source, logical, |value| {
            unrestricted || target.admits(value)
        })
        .unwrap_or(false)
    }

    pub(crate) fn scalar_value(
        &self,
        slots: FrameSlots,
        logical: usize,
    ) -> Option<(ValueType, u64)> {
        let window = self.window(slots)?;
        let location = window.location(logical)?;
        let slot = location.operand.scalar()?;
        Some((location.representation, self.payload(&window.ranges, slot)?))
    }

    pub(crate) fn check_managed(
        &self,
        slots: FrameSlots,
        logical: usize,
        check: impl FnOnce(&Value) -> bool,
    ) -> Option<bool> {
        let window = self.window(slots)?;
        let location = window.location(logical)?;
        if let Some(slot) = location.operand.scalar() {
            self.payload(&window.ranges, slot)?;
            return Some(true);
        }
        self.with_value(slots, logical, check)
    }

    pub(crate) fn set_scalar(
        &mut self,
        slots: FrameSlots,
        logical: usize,
        representation: ValueType,
        bits: u64,
    ) -> Option<()> {
        let window = self.window(slots)?;
        let location = window.location(logical)?;
        if location.operand.managed() {
            return self.set(slots, logical, scalar::decode(representation, bits)?);
        }
        if !location.admits_payload(representation, bits) {
            return None;
        }
        let index = window.ranges.scalars.start + location.operand.index();
        *self.payloads.get_mut(index)? = bits;
        *self.initialized.get_mut(index)? = true;
        Some(())
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
    pub(crate) fn get(&self, slots: FrameSlots) -> Option<Vec<Value>> {
        let window = self.window(slots)?;
        let count = window
            .registers
            .as_ref()
            .map_or(window.ranges.managed.len(), |layout| layout.locations.len());
        (0..count)
            .map(|index| self.with_value(slots, index, Value::clone))
            .collect()
    }

    pub(crate) fn ranges(&self, slots: FrameSlots) -> Option<WindowRanges> {
        Some(self.window(slots)?.ranges.clone())
    }

    pub(crate) fn with_value<R>(
        &self,
        slots: FrameSlots,
        logical: usize,
        read: impl FnOnce(&Value) -> R,
    ) -> Option<R> {
        let window = self.window(slots)?;
        let location = window.location(logical)?;
        let slot = location.operand;
        if slot.managed() {
            return self
                .values
                .get(window.ranges.managed.start + slot.index())
                .map(read);
        }
        let index = window.ranges.scalars.start + slot.index();
        if !self.initialized.get(index).copied()? {
            // Debugger/native inspection preserves unavailable Unit; execution
            // uses payload(), which rejects uninitialized scalar operands.
            return Some(read(&Value::Unit));
        }
        scalar::decode(location.representation, *self.payloads.get(index)?)
            .as_ref()
            .map(read)
    }

    pub(crate) fn set(&mut self, slots: FrameSlots, logical: usize, value: Value) -> Option<()> {
        let window = self.window(slots)?;
        let location = window.location(logical)?;
        if window.registers.is_some() && !location.admits(&value) {
            return None;
        }
        let slot = location.operand;
        if slot.managed() {
            let index = window.ranges.managed.start + slot.index();
            *self.values.get_mut(index)? = value;
        } else {
            let index = window.ranges.scalars.start + slot.index();
            *self.payloads.get_mut(index)? = scalar::encode(&value)?;
            *self.initialized.get_mut(index)? = true;
        }
        Some(())
    }

    #[inline]
    pub(crate) fn payload(&self, ranges: &WindowRanges, slot: ScalarSlot) -> Option<u64> {
        if slot.index() >= ranges.scalars.len() {
            return None;
        }
        let index = ranges.scalars.start + slot.index();
        self.initialized
            .get(index)
            .copied()?
            .then(|| self.payloads[index])
    }

    #[inline]
    pub(crate) fn write_payload(
        &mut self,
        ranges: &WindowRanges,
        slot: ScalarSlot,
        value: u64,
    ) -> Option<()> {
        if slot.index() >= ranges.scalars.len() {
            return None;
        }
        let index = ranges.scalars.start + slot.index();
        *self.payloads.get_mut(index)? = value;
        *self.initialized.get_mut(index)? = true;
        Some(())
    }

    pub(crate) fn release(&mut self, slots: FrameSlots) -> Option<()> {
        let ranges = self.window(slots)?.ranges.clone();
        self.values[ranges.managed].fill(Value::Unit);
        self.initialized[ranges.scalars].fill(false);
        self.windows[slots.index] = None;
        while self.windows.last().is_some_and(Option::is_none) {
            self.windows.pop();
        }
        let window = self.windows.last().and_then(Option::as_ref);
        self.values
            .truncate(window.map_or(0, |w| w.ranges.managed.end));
        let end = window.map_or(0, |w| w.ranges.scalars.end);
        self.payloads.truncate(end);
        self.initialized.truncate(end);
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
        values.with_value(self, index, read)
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
        values.set(self, index, value)
    }
}

#[cfg(test)]
mod tests;
