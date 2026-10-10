//! Runtime-owned execution windows, independent of persistent host root leases.
use kagari_abi::representation::ValueType;

pub(crate) mod operands;
pub(crate) mod scalar;

use crate::{
    error::RuntimeError,
    execution_metadata::MetadataRoot,
    frame::{
        arguments::FrameArguments,
        types::TypeEnvironment,
        values::operands::{read_operand, write_operand},
    },
    gc::GcHeap,
    module::{
        LoadedModule,
        execution::{
            OperandSlot, ScalarSlot,
            layout::{FrameLayout, Location},
        },
    },
    native::application::NativeApplication,
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
    native_application: Option<Arc<NativeApplication>>,
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
    allocation_order: Vec<usize>,
}

impl Default for ExecutionValues {
    fn default() -> Self {
        static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);
        Self {
            owner: NEXT_OWNER
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
                .expect("execution storage identity exhausted"),
            next_generation: 0,
            values: Vec::new(),
            payloads: Vec::new(),
            initialized: Vec::new(),
            windows: Vec::new(),
            allocation_order: Vec::new(),
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
            for transfer in operands {
                let target = transfer.target;
                let capacity = if target.operand.managed() {
                    managed_count
                } else {
                    scalar_count
                };
                if target.operand.index() >= capacity
                    || !self.admits_transfer(source, transfer.source, target)
                {
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
        self.allocation_order
            .try_reserve(1)
            .map_err(|_| RuntimeError::resource_limit("execution frame order"))?;
        let scalar_start = self.payloads.len();
        let managed_start = self.values.len();
        self.payloads.resize(scalar_start + scalar_count, 0);
        self.initialized.resize(scalar_start + scalar_count, false);
        self.values
            .resize(managed_start + managed_count, Value::Unit);
        self.next_generation = generation;
        let slots = FrameSlots {
            owner: self.owner,
            index: self
                .windows
                .iter()
                .position(Option::is_none)
                .unwrap_or(self.windows.len()),
            generation,
        };
        let window = Some(Window {
            generation,
            ranges: WindowRanges {
                scalars: scalar_start..scalar_start + scalar_count,
                managed: managed_start..managed_start + managed_count,
            },
            program,
            environment,
            native_application: None,
            registers,
        });
        if slots.index == self.windows.len() {
            self.windows.push(window);
        } else {
            self.windows[slots.index] = window;
        }
        self.allocation_order.push(slots.index);
        if let Some((source, operands)) = arguments.window() {
            let ranges = self.ranges(slots).expect("published window");
            for transfer in operands {
                // Indices survive bank growth; source and destination are disjoint.
                if let Some((_, bits)) = self.scalar_location(source, transfer.source)
                    && let Some(target) = transfer.target.operand.scalar()
                {
                    self.write_payload(&ranges, target, bits)
                        .expect("admitted scalar transfer");
                } else {
                    let value = self
                        .with_location(source, transfer.source, Value::clone)
                        .expect("checked source window");
                    self.set_location(slots, transfer.target, value)
                        .expect("admitted argument");
                }
            }
        } else {
            for (index, value) in arguments.iter().enumerate() {
                self.set(slots, argument_offset + index, *value)
                    .expect("admitted argument");
            }
        }
        Ok(slots)
    }

    fn admits_transfer(&self, source: FrameSlots, location: Location, target: Location) -> bool {
        if let Some((representation, bits)) = self.scalar_location(source, location)
            && !target.operand.managed()
        {
            return target.admits_payload(representation, bits);
        }
        self.with_location(source, location, |value| target.admits(value))
            .unwrap_or(false)
    }

    fn scalar_location(&self, slots: FrameSlots, location: Location) -> Option<(ValueType, u64)> {
        let window = self.window(slots)?;
        let slot = location.operand.scalar()?;
        Some((location.representation, self.payload(&window.ranges, slot)?))
    }

    pub(crate) fn check_managed_location(
        &self,
        slots: FrameSlots,
        location: Location,
        check: impl FnOnce(&Value) -> bool,
    ) -> Option<bool> {
        let window = self.window(slots)?;
        if let Some(slot) = location.operand.scalar() {
            self.payload(&window.ranges, slot)?;
            return Some(true);
        }
        self.with_location(slots, location, check)
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
        self.set_scalar_location(slots, location, representation, bits)
    }

    pub(crate) fn set_scalar_location(
        &mut self,
        slots: FrameSlots,
        location: Location,
        representation: ValueType,
        bits: u64,
    ) -> Option<()> {
        let window = self.window(slots)?;
        if location.operand.managed() {
            return self.set_location(slots, location, scalar::decode(representation, bits)?);
        }
        if !location.admits_payload(representation, bits) {
            return None;
        }
        let ranges = window.ranges.clone();
        self.write_payload(&ranges, location.operand.scalar()?, bits)
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

    /// Only checked await facts may clear dead storage. Ordinary typed writes
    /// cannot put Unit into another representation; this discards unused roots.
    pub(crate) fn retain_managed(&mut self, slots: FrameSlots, retained: &[u64]) -> Option<()> {
        let range = self.window(slots)?.ranges.managed.clone();
        if retained.len() != range.len().div_ceil(64) {
            return None;
        }
        for (index, value) in self.values[range].iter_mut().enumerate() {
            if retained[index / 64] & (1 << (index % 64)) == 0 {
                *value = Value::Unit;
            }
        }
        Some(())
    }

    pub(crate) fn with_value<R>(
        &self,
        slots: FrameSlots,
        logical: usize,
        read: impl FnOnce(&Value) -> R,
    ) -> Option<R> {
        let window = self.window(slots)?;
        let location = window.location(logical)?;
        self.with_location(slots, location, read)
    }

    pub(crate) fn with_location<R>(
        &self,
        slots: FrameSlots,
        location: Location,
        read: impl FnOnce(&Value) -> R,
    ) -> Option<R> {
        let ranges = &self.window(slots)?.ranges;
        let value = read_operand(
            self.values.get(ranges.managed.clone())?,
            self.payloads.get(ranges.scalars.clone())?,
            self.initialized.get(ranges.scalars.clone())?,
            location,
        )?;
        Some(read(&value))
    }

    pub(crate) fn set(&mut self, slots: FrameSlots, logical: usize, value: Value) -> Option<()> {
        let window = self.window(slots)?;
        let location = window.location(logical)?;
        self.set_location(slots, location, value)
    }

    pub(crate) fn set_location(
        &mut self,
        slots: FrameSlots,
        location: Location,
        value: Value,
    ) -> Option<()> {
        let ranges = self.window(slots)?.ranges.clone();
        write_operand(
            self.values.get_mut(ranges.managed)?,
            self.payloads.get_mut(ranges.scalars.clone())?,
            self.initialized.get_mut(ranges.scalars)?,
            location,
            value,
        )
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
        let order = self
            .allocation_order
            .iter()
            .rposition(|index| *index == slots.index)?;
        let tail = order + 1 == self.allocation_order.len();
        self.allocation_order.remove(order);
        self.windows[slots.index] = None;
        if tail {
            // Ordinary returns retire the most recently allocated window. Older
            // roots precede both ranges, including zero-width banks and reused IDs.
            self.values.truncate(ranges.managed.start);
            self.payloads.truncate(ranges.scalars.start);
            self.initialized.truncate(ranges.scalars.start);
        } else {
            // Independent roots can finish in any order. Compact the banks while
            // retaining generational window identities; no cursor may be borrowed
            // during release. Leaving interior holes would grow with historical work.
            self.values.drain(ranges.managed.clone());
            self.payloads.drain(ranges.scalars.clone());
            self.initialized.drain(ranges.scalars.clone());
            for window in self.windows.iter_mut().flatten() {
                if window.ranges.managed.start >= ranges.managed.end {
                    window.ranges.managed.start -= ranges.managed.len();
                    window.ranges.managed.end -= ranges.managed.len();
                }
                if window.ranges.scalars.start >= ranges.scalars.end {
                    window.ranges.scalars.start -= ranges.scalars.len();
                    window.ranges.scalars.end -= ranges.scalars.len();
                }
            }
        }
        while self.windows.last().is_some_and(Option::is_none) {
            self.windows.pop();
        }
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
            if let Some(application) = &window.native_application {
                roots.push(MetadataRoot::NativeApplication(application.clone()));
            }
            roots.push(MetadataRoot::Program(window.program.clone()));
            if let Some(environment) = &window.environment {
                roots.push(MetadataRoot::Environment(environment.id));
            }
        }
    }
}

impl FrameSlots {
    pub(crate) fn publish_native_application(
        self,
        heap: &GcHeap,
        application: Arc<NativeApplication>,
    ) -> Result<(), RuntimeError> {
        let invalid = || RuntimeError::module_validation("native application window");
        let mut values = heap
            .resources()
            .frame_values
            .try_borrow_mut()
            .map_err(|_| invalid())?;
        let window = values.window(self).ok_or_else(invalid)?;
        if window.program.program_identity() != application.owner.program_identity()
            || window.program.key() != application.owner.key()
            || window
                .environment
                .as_ref()
                .map(|environment| environment.id)
                != Some(application.environment.id)
            || window.native_application.is_some()
        {
            return Err(invalid());
        }
        values.windows[self.index]
            .as_mut()
            .ok_or_else(invalid)?
            .native_application = Some(application);
        Ok(())
    }

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

    pub(crate) fn set_location(
        self,
        heap: &GcHeap,
        location: Location,
        value: Value,
    ) -> Option<()> {
        heap.ensure_execution_allowed().ok()?;
        if !heap.validate_value(&value) {
            return None;
        }
        let mut values = heap.resources().frame_values.try_borrow_mut().ok()?;
        values.set_location(self, location, value)
    }
}

#[cfg(test)]
mod tests;
