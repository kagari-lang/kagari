//! Scoped native arguments/results share the authoritative execution root bank.

use crate::{
    Runtime,
    error::RuntimeError,
    frame::{
        arguments::{FrameArguments, InvocationArguments, WindowArguments},
        types::arguments::TypeArgument,
        values::{FrameMetadata, FrameSlots},
    },
    gc::roots::RootSet,
    module::{ExecutableModule, execution::layout::FrameLayout},
    native::{
        binding::NativeResult,
        conversion::{FromKagari, IntoKagari, context::ConversionContext},
    },
    value::Value,
};
use std::ptr;

/// A scoped projection of argument slots and one result slot.
///
/// Native adapters borrow their active frame; host conversions own a temporary
/// window. Both are traced by the execution bank through nested GC/reentry. Only
/// an owning guard releases a window. No payload/bank reference escapes a read.
pub struct CallSlots<'runtime> {
    runtime: &'runtime Runtime,
    source: WindowArguments<'runtime>,
    published: usize,
    result_slot: usize,
    owned: bool,
}

/// A bounded argument projection used by custom Rust argument-pack adapters.
pub struct ArgumentWriter<'slots, 'signature, 'runtime> {
    slots: &'slots mut CallSlots<'runtime>,
    expected: &'signature [TypeArgument],
    start: usize,
}

/// Read-only coordinates into a frozen, call-owned argument snapshot.
/// No bank or payload borrow survives an individual read or user conversion.
#[derive(Clone, Copy)]
pub struct ArgumentReader<'slots, 'runtime> {
    slots: &'slots CallSlots<'runtime>,
    start: usize,
    count: usize,
}

impl<'runtime> CallSlots<'runtime> {
    pub(crate) fn new(
        runtime: &'runtime Runtime,
        owner: &ExecutableModule,
        count: usize,
    ) -> NativeResult<Self> {
        runtime.validate_executable_module(owner)?;
        let width = count
            .checked_add(1)
            .ok_or_else(|| RuntimeError::resource_limit("argument window size"))?;
        let slots = runtime
            .resources()
            .frame_values
            .try_borrow_mut()
            .map_err(|_| RuntimeError::module_validation("argument bank is borrowed"))?
            .allocate(
                width,
                &[],
                &FrameArguments::plain(&[]),
                FrameMetadata {
                    program: owner.clone(),
                    environment: None,
                    invocation: None,
                },
                None,
            )?;
        Ok(Self {
            runtime,
            source: WindowArguments::contiguous(slots, count),
            published: 0,
            result_slot: count,
            owned: true,
        })
    }

    /// Number of explicit arguments, excluding the reserved result slot.
    pub fn len(&self) -> usize {
        self.source.len()
    }

    /// Whether this call has no explicit arguments.
    pub fn is_empty(&self) -> bool {
        self.source.len() == 0
    }

    /// Copy an argument while this guard keeps its managed target alive.
    /// Returns None for an absent slot or an invalidated execution window.
    /// The copied value acquires no independent root lease.
    pub fn get(&self, index: usize) -> Option<Value> {
        if index >= self.source.len() || index >= self.published {
            return None;
        }
        self.source.slots().with_location(
            self.runtime.gc(),
            self.source.location(index),
            Value::clone,
        )
    }

    /// Lend read-only coordinates for synchronous backend admission.
    /// The source cannot outlive this guard and holds no bank borrow.
    pub fn arguments(&self) -> InvocationArguments<'_> {
        InvocationArguments {
            source: FrameArguments::frame(&self.source),
        }
    }

    pub(crate) fn writer<'slots, 'signature>(
        &'slots mut self,
        expected: &'signature [TypeArgument],
    ) -> NativeResult<ArgumentWriter<'slots, 'signature, 'runtime>> {
        if expected.len() != self.source.len() {
            return Err(RuntimeError::module_validation("argument writer count"));
        }
        Ok(ArgumentWriter {
            slots: self,
            expected,
            start: 0,
        })
    }

    pub(crate) fn snapshot(
        runtime: &'runtime Runtime,
        owner: &ExecutableModule,
        count: usize,
        read: impl FnMut(usize) -> NativeResult<Value>,
    ) -> NativeResult<Self> {
        Self::snapshot_into(Self::new(runtime, owner, count)?, read)
    }

    /// Borrow the active Native frame. Its parameters stay frozen until return;
    /// this view owns neither a second root window nor the frame's cleanup.
    pub(crate) fn native(
        runtime: &'runtime Runtime,
        frame: FrameSlots,
        layout: &'runtime FrameLayout,
        count: usize,
        adapted_result: bool,
    ) -> Self {
        Self {
            runtime,
            source: WindowArguments::mapped(frame, &layout.locations[1..1 + count]),
            published: count,
            result_slot: if adapted_result { count + 1 } else { 0 },
            owned: false,
        }
    }

    fn snapshot_into(
        mut slots: Self,
        mut read: impl FnMut(usize) -> NativeResult<Value>,
    ) -> NativeResult<Self> {
        for index in 0..slots.len() {
            slots.set(index, read(index)?)?;
        }
        slots.finish_arguments()?;
        Ok(slots)
    }

    pub(crate) fn reader(&self) -> ArgumentReader<'_, 'runtime> {
        ArgumentReader {
            slots: self,
            start: 0,
            count: self.len(),
        }
    }

    pub(crate) fn set(&mut self, index: usize, value: Value) -> NativeResult<()> {
        self.runtime.gc().ensure_execution_allowed()?;
        self.ensure_write(index)?;
        self.source
            .slots()
            .set(
                self.runtime.gc(),
                if index == self.source.len() {
                    self.result_slot
                } else {
                    index
                },
                value,
            )
            .ok_or_else(|| RuntimeError::module_validation("argument slot publication"))?;
        self.published += 1;
        Ok(())
    }

    pub(crate) fn finish_arguments(&self) -> NativeResult<()> {
        self.runtime.gc().ensure_execution_allowed()?;
        if self.published != self.source.len() {
            return Err(RuntimeError::module_validation("incomplete argument pack"));
        }
        Ok(())
    }

    fn ensure_write(&self, index: usize) -> NativeResult<()> {
        if index > self.source.len() || index != self.published {
            return Err(RuntimeError::module_validation(
                "arguments must be published once in source order",
            ));
        }
        Ok(())
    }

    pub(crate) fn belongs_to(&self, runtime: &Runtime) -> bool {
        ptr::eq(self.runtime, runtime)
    }

    pub(crate) fn result_index(&self) -> usize {
        self.source.len()
    }

    pub(crate) fn read_result(&self) -> NativeResult<Value> {
        if self.published <= self.source.len() {
            return Err(RuntimeError::module_validation("uninitialized result slot"));
        }
        self.source
            .slots()
            .get(self.runtime.gc(), self.result_slot)
            .ok_or_else(|| RuntimeError::module_validation("result slot"))
    }

    pub(crate) fn values(&self) -> NativeResult<Vec<Value>> {
        let mut values = Vec::new();
        values
            .try_reserve_exact(self.source.len())
            .map_err(|_| self.runtime.resources().limit("typed call arguments"))?;
        for index in 0..self.source.len() {
            values.push(
                self.get(index)
                    .ok_or_else(|| RuntimeError::module_validation("argument slot"))?,
            );
        }
        Ok(values)
    }

    pub(crate) fn into_owned(self) -> NativeResult<(RootSet, Vec<Value>)> {
        let values = self.values()?;
        let roots = self
            .runtime
            .gc()
            .root_execution_values(values.clone())
            .ok_or_else(|| RuntimeError::module_validation("owned argument retention"))?;
        Ok((roots, values))
    }
}

impl<'slots, 'runtime> ArgumentReader<'slots, 'runtime> {
    /// Number of arguments in this projection, excluding the result slot.
    pub fn len(&self) -> usize {
        self.count
    }

    /// Whether this projection has no arguments.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Copy a protected argument without an independent root lease.
    /// Rejects invalid indices and invalidated owner/window generations.
    pub fn get(&self, index: usize) -> NativeResult<Value> {
        if index >= self.count {
            return Err(RuntimeError::module_validation("argument reader index"));
        }
        self.slots
            .get(self.start + index)
            .ok_or_else(|| RuntimeError::module_validation("argument reader slot"))
    }

    /// Check a custom converter's mapping, then decode a protected argument.
    pub fn decode<T: FromKagari>(
        &self,
        cx: &mut ConversionContext<'runtime>,
        expected: &TypeArgument,
        index: usize,
    ) -> NativeResult<T> {
        cx.check_type::<T>(expected)?;
        self.decode_prepared(cx, expected, index)
    }

    pub(crate) fn decode_prepared<T: FromKagari>(
        &self,
        cx: &mut ConversionContext<'runtime>,
        expected: &TypeArgument,
        index: usize,
    ) -> NativeResult<T> {
        if !self.slots.belongs_to(cx.runtime()) {
            return Err(RuntimeError::module_validation("foreign argument reader"));
        }
        let value = self.get(index)?;
        cx.decode_slot(expected, &value)
    }

    pub(crate) fn check_values(
        &self,
        cx: &ConversionContext<'_>,
        expected: &[TypeArgument],
    ) -> NativeResult<()> {
        if !self.slots.belongs_to(cx.runtime()) || expected.len() != self.count {
            return Err(RuntimeError::module_validation("argument reader signature"));
        }
        for (index, ty) in expected.iter().enumerate() {
            cx.check_value(ty, &self.get(index)?)?;
        }
        Ok(())
    }

    pub(crate) fn suffix(&self, start: usize) -> NativeResult<Self> {
        let count = self
            .count
            .checked_sub(start)
            .ok_or_else(|| RuntimeError::module_validation("argument reader suffix"))?;
        Ok(Self {
            slots: self.slots,
            start: self.start + start,
            count,
        })
    }
}

impl Drop for CallSlots<'_> {
    fn drop(&mut self) {
        if !self.owned {
            return;
        }
        match self.runtime.resources().frame_values.try_borrow_mut() {
            Ok(mut storage) => {
                if storage.release(self.source.slots()).is_none() {
                    self.runtime
                        .resources()
                        .quarantine("argument window release");
                }
            }
            Err(_) => {
                self.runtime
                    .resources()
                    .quarantine("argument bank borrowed during cleanup");
            }
        }
    }
}

impl<'signature, 'runtime> ArgumentWriter<'_, 'signature, 'runtime> {
    /// Convert and publish the next argument before its conversion safepoint.
    ///
    /// Indices start at zero within this pack and must be written exactly once,
    /// in order. Mapping, value, conversion and Runtime errors propagate; earlier
    /// arguments remain protected until the owning call window is dropped.
    pub fn encode<T: IntoKagari>(
        &mut self,
        cx: &mut ConversionContext<'runtime>,
        index: usize,
        value: T,
    ) -> NativeResult<()> {
        self.slots.runtime.gc().ensure_execution_allowed()?;
        let expected = self
            .expected
            .get(index)
            .ok_or_else(|| RuntimeError::module_validation("argument writer index"))?;
        self.slots.ensure_write(self.start + index)?;
        cx.check_type::<T>(expected)?;
        self.encode_prepared(cx, index, value)
    }

    pub(crate) fn encode_prepared<T: IntoKagari>(
        &mut self,
        cx: &mut ConversionContext<'runtime>,
        index: usize,
        value: T,
    ) -> NativeResult<()> {
        self.slots.runtime.gc().ensure_execution_allowed()?;
        let expected = self
            .expected
            .get(index)
            .ok_or_else(|| RuntimeError::module_validation("argument writer index"))?;
        self.slots.ensure_write(self.start + index)?;
        cx.encode_into(expected, value, self.slots, self.start + index)
    }

    pub(crate) fn suffix(
        &mut self,
        start: usize,
    ) -> NativeResult<ArgumentWriter<'_, 'signature, 'runtime>> {
        let expected = self
            .expected
            .get(start..)
            .ok_or_else(|| RuntimeError::module_validation("argument writer suffix"))?;
        Ok(ArgumentWriter {
            slots: self.slots,
            expected,
            start: self.start + start,
        })
    }
}
