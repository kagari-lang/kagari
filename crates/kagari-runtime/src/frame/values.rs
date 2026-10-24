//! Runtime-owned execution windows, independent of persistent host root leases.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};

use kagari_abi::representation::ValueType;

pub(crate) mod operands;
mod windows;

use crate::{
    error::RuntimeError,
    execution_metadata::MetadataRoot,
    frame::{
        arguments::{ArgumentSource, FrameArguments},
        types::TypeEnvironment,
        values::{
            operands::{read_operand, write_operand},
            windows::Windows,
        },
    },
    gc::GcHeap,
    module::{
        ExecutableModule,
        execution::{
            OperandSlot, ScalarSlot,
            layout::{FrameLayout, Location},
        },
    },
    native::application::NativeApplication,
    objects::invocation::MethodInvocation,
    value::{Value, scalar},
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

/// Dependencies published atomically with the callee's parameter roots.
pub(crate) struct FrameMetadata {
    pub program: ExecutableModule,
    pub environment: Option<TypeEnvironment>,
    pub invocation: Option<MethodInvocation>,
}

#[derive(Debug)]
struct Window {
    native_application: Option<Arc<NativeApplication>>,
    invocation: Option<MethodInvocation>,
    generation: u64,
    ranges: WindowRanges,
    program: ExecutableModule,
    environment: Option<TypeEnvironment>,
    slot_layout: Option<Arc<FrameLayout>>,
}

#[derive(Debug, Clone)]
pub(crate) struct WindowRanges {
    pub scalars: Range<usize>,
    pub managed: Range<usize>,
}

impl Window {
    fn location(&self, logical: usize) -> Option<Location> {
        match &self.slot_layout {
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
    windows: Windows,
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
            windows: Windows::default(),
            allocation_order: Vec::new(),
        }
    }
}

impl ExecutionValues {
    pub(crate) fn allocate(
        &mut self,
        count: usize,
        parameter_slots: &[usize],
        arguments: &FrameArguments<'_>,
        metadata: FrameMetadata,
        slot_layout: Option<Arc<FrameLayout>>,
    ) -> Result<FrameSlots, RuntimeError> {
        if parameter_slots.len() != arguments.len()
            || parameter_slots.iter().any(|&slot| slot >= count)
        {
            return Err(RuntimeError::module_validation("frame argument window"));
        }
        let (scalar_count, managed_count) = match &slot_layout {
            Some(layout) if layout.locations.len() == count => {
                (layout.scalar_count, layout.managed_count)
            }
            Some(_) => return Err(RuntimeError::module_validation("frame layout size")),
            None => (0, count),
        };
        let destination = |index: usize| {
            slot_layout.as_ref().map_or(
                Some(Location {
                    operand: OperandSlot::new(index, true),
                    representation: ValueType::Generic,
                    semantic: None,
                }),
                |layout| layout.location(index),
            )
        };
        // One exclusive storage borrow covers validation, reserve, transfer and
        // publication. Indices survive vector growth; no window can retire or
        // move while these source ranges are used.
        let source_ranges = arguments
            .window()
            .map(|slots| {
                self.ranges(slots)
                    .ok_or_else(|| RuntimeError::module_validation("invalid argument window"))
            })
            .transpose()?;
        // Admission is transactional: reject an invalid argument before growing
        // any bank or publishing a frame window. No partial roots can escape.
        for (index, source) in arguments.iter().enumerate() {
            let target = destination(parameter_slots[index]).expect("checked argument range");
            let capacity = if target.operand.managed() {
                managed_count
            } else {
                scalar_count
            };
            let admitted = match source {
                ArgumentSource::Value(value) => slot_layout.is_none() || target.admits(value),
                ArgumentSource::Window(location) => self.admits_transfer(
                    source_ranges.as_ref().expect("admitted source window"),
                    location,
                    target,
                ),
            };
            if target.operand.index() >= capacity || !admitted {
                return Err(RuntimeError::module_validation(
                    "invalid frame argument type or window",
                ));
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
            .reserve_slot()
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
        #[cfg(feature = "execution-diagnostics")]
        {
            diagnostics::record(Event::WindowAllocation);
            diagnostics::record_amount(Event::WindowSlotsExamined, 0);
            diagnostics::record_amount(
                Event::BankBytesInitialized,
                (scalar_count * (size_of::<u64>() + size_of::<bool>())
                    + managed_count * size_of::<Value>()) as u64,
            );
            diagnostics::record_amount(Event::ArgumentTransfer, arguments.len() as u64);
        }
        let ranges = WindowRanges {
            scalars: scalar_start..scalar_start + scalar_count,
            managed: managed_start..managed_start + managed_count,
        };
        // Exclusive bank access excludes GC and callbacks. Initialize reserved
        // parameter ranges before publishing their identity and metadata roots.
        for (index, source) in arguments.iter().enumerate() {
            let target =
                destination(parameter_slots[index]).expect("admitted argument destination");
            match source {
                ArgumentSource::Value(value) => {
                    self.write_range(&ranges, target, *value)
                        .expect("admitted argument");
                }
                ArgumentSource::Window(location) => {
                    let source = source_ranges.as_ref().expect("admitted source window");
                    // Indices survive bank growth; source and destination are disjoint.
                    if let Some(bits) = location
                        .operand
                        .scalar()
                        .and_then(|slot| self.payload(source, slot))
                        && let Some(target) = target.operand.scalar()
                    {
                        self.write_payload(&ranges, target, bits)
                            .expect("admitted scalar transfer");
                    } else {
                        let value = self
                            .with_range(source, location, Value::clone)
                            .expect("checked source window");
                        self.write_range(&ranges, target, value)
                            .expect("admitted argument");
                    }
                }
            }
        }
        let index = self.windows.insert(Window {
            generation,
            ranges,
            program: metadata.program,
            environment: metadata.environment,
            invocation: metadata.invocation,
            native_application: None,
            slot_layout,
        });
        let slots = FrameSlots {
            owner: self.owner,
            index,
            generation,
        };
        self.allocation_order.push(slots.index);
        Ok(slots)
    }

    fn admits_transfer(&self, source: &WindowRanges, location: Location, target: Location) -> bool {
        if let Some(bits) = location
            .operand
            .scalar()
            .and_then(|slot| self.payload(source, slot))
            && !target.operand.managed()
        {
            return target.admits_payload(location.representation, bits);
        }
        self.with_range(source, location, |value| target.admits(value))
            .unwrap_or(false)
    }

    pub(crate) fn scalar_location(
        &self,
        slots: FrameSlots,
        location: Location,
    ) -> Option<(ValueType, u64)> {
        let window = self.window(slots)?;
        let slot = location.operand.scalar()?;
        Some((location.representation, self.payload(&window.ranges, slot)?))
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
            .get(slots.index)
            .filter(|window| window.generation == slots.generation)
    }

    #[cfg(test)]
    pub(crate) fn get(&self, slots: FrameSlots) -> Option<Vec<Value>> {
        let window = self.window(slots)?;
        let count = window
            .slot_layout
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
        self.with_range(ranges, location, read)
    }

    fn with_range<R>(
        &self,
        ranges: &WindowRanges,
        location: Location,
        read: impl FnOnce(&Value) -> R,
    ) -> Option<R> {
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
        self.write_range(&ranges, location, value)
    }

    fn write_range(
        &mut self,
        ranges: &WindowRanges,
        location: Location,
        value: Value,
    ) -> Option<()> {
        write_operand(
            self.values.get_mut(ranges.managed.clone())?,
            self.payloads.get_mut(ranges.scalars.clone())?,
            self.initialized.get_mut(ranges.scalars.clone())?,
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
        self.windows.retire(slots.index);
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
            #[cfg(feature = "execution-diagnostics")]
            diagnostics::record_amount(
                Event::BankBytesMoved,
                ((self.values.len() - ranges.managed.end) * size_of::<Value>()
                    + (self.payloads.len() - ranges.scalars.end)
                        * (size_of::<u64>() + size_of::<bool>())) as u64,
            );
            self.values.drain(ranges.managed.clone());
            self.payloads.drain(ranges.scalars.clone());
            self.initialized.drain(ranges.scalars.clone());
            let managed = ranges.managed.len();
            let scalars = ranges.scalars.len();
            if managed != 0 || scalars != 0 {
                // Banks append in allocation order, independently of reused slot
                // identities. Only this surviving suffix follows the removed
                // ranges; earlier windows and vacant slots cannot move.
                for &index in &self.allocation_order[order..] {
                    let window = self.windows.get_mut(index).expect("live allocation order");
                    window.ranges.managed.start -= managed;
                    window.ranges.managed.end -= managed;
                    window.ranges.scalars.start -= scalars;
                    window.ranges.scalars.end -= scalars;
                }
            }
        }
        Some(())
    }

    pub(crate) fn active_windows(&self) -> usize {
        self.windows.iter().count()
    }

    pub(crate) fn append_values(&self, roots: &mut Vec<Value>) {
        roots.extend_from_slice(&self.values);
    }

    pub(crate) fn append_metadata(&self, roots: &mut Vec<MetadataRoot>) {
        for window in self.windows.iter() {
            if let Some(invocation) = window.invocation {
                invocation.append_metadata(roots);
            }
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
        values
            .windows
            .get_mut(self.index)
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

    pub(crate) fn with_location<R>(
        self,
        heap: &GcHeap,
        location: Location,
        read: impl FnOnce(&Value) -> R,
    ) -> Option<R> {
        let values = heap.resources().frame_values.try_borrow().ok()?;
        values.with_location(self, location, read)
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
