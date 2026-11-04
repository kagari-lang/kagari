//! One synchronous conversion scope protects unpublished values and exact versions.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};

use crate::{
    Runtime,
    error::RuntimeError,
    frame::{call_slots::CallSlots, types::arguments::TypeArgument},
    gc::{HeapObjectId, roots::RootedValue},
    module::{ExecutableModule, ModuleEpochRetention, retention::ProgramLease},
    native::{
        binding::NativeResult,
        context::CallContext,
        conversion::{FromKagari, IntoKagari, KagariType},
        function_handle::PreparedFunction,
        types::Type,
    },
    value::Value,
};
use std::{
    collections::HashSet,
    ops::{Deref, DerefMut},
};

#[derive(Debug, Clone, Copy)]
pub struct ConversionLimits {
    pub max_depth: usize,
    pub max_nodes: usize,
    pub max_string_bytes: usize,
}

impl Default for ConversionLimits {
    fn default() -> Self {
        Self {
            max_depth: 64,
            max_nodes: 1_000_000,
            max_string_bytes: 16 * 1024 * 1024,
        }
    }
}

/// Borrow the owning runtime only for synchronous conversion. Values returned by
/// `encode` have independent root leases; scratch roots expire on every exit.
pub struct ConversionContext<'runtime> {
    runtime: &'runtime Runtime,
    owner: &'runtime ExecutableModule,
    _program: Option<ProgramLease>,
    limits: ConversionLimits,
    depth: usize,
    nodes: usize,
    string_bytes: usize,
    active: HashSet<HeapObjectId>,
    roots: Vec<RootedValue>,
}

struct Frame<'scope, 'runtime> {
    context: &'scope mut ConversionContext<'runtime>,
    root_base: usize,
    identity: Option<HeapObjectId>,
}

impl<'runtime> Deref for Frame<'_, 'runtime> {
    type Target = ConversionContext<'runtime>;
    fn deref(&self) -> &Self::Target {
        self.context
    }
}

impl DerefMut for Frame<'_, '_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.context
    }
}

impl Drop for Frame<'_, '_> {
    fn drop(&mut self) {
        self.context.roots.truncate(self.root_base);
        self.context.depth -= 1;
        if let Some(identity) = self.identity {
            self.context.active.remove(&identity);
        }
    }
}

impl<'runtime> ConversionContext<'runtime> {
    pub fn new(
        runtime: &'runtime Runtime,
        owner: &'runtime ExecutableModule,
    ) -> NativeResult<Self> {
        Self::with_limits(runtime, owner, ConversionLimits::default())
    }

    pub fn with_limits(
        runtime: &'runtime Runtime,
        owner: &'runtime ExecutableModule,
        limits: ConversionLimits,
    ) -> NativeResult<Self> {
        let mut context = Self::borrowed(runtime, owner)?;
        let program = runtime
            .retain_program(owner, ModuleEpochRetention::ActiveCall)
            .ok_or_else(|| RuntimeError::module_validation("conversion program retention"))?;
        context._program = Some(program);
        context.limits = limits;
        Ok(context)
    }

    /// The synchronous call already retains its exact program through the
    /// execution frame or selected-call lease. Only escaping handles add leases.
    pub(crate) fn in_native_call(call: &'runtime CallContext<'_>) -> NativeResult<Self> {
        Self::borrowed(call.runtime, call.owner)
    }

    /// Borrow the exact selection's existing lease for synchronous conversion.
    /// The context cannot outlive that selection, including during GC/reentry.
    pub(crate) fn in_prepared_call(
        runtime: &'runtime Runtime,
        function: &'runtime PreparedFunction,
    ) -> NativeResult<Self> {
        Self::borrowed(runtime, function.owner())
    }

    fn borrowed(
        runtime: &'runtime Runtime,
        owner: &'runtime ExecutableModule,
    ) -> NativeResult<Self> {
        #[cfg(feature = "execution-diagnostics")]
        diagnostics::record(Event::ConversionContext);
        runtime.gc().ensure_no_native_borrow()?;
        runtime.resources().ensure_execution_allowed()?;
        runtime.validate_executable_module(owner)?;
        Ok(Self {
            runtime,
            owner,
            _program: None,
            limits: ConversionLimits::default(),
            depth: 0,
            nodes: 0,
            string_bytes: 0,
            active: HashSet::new(),
            roots: Vec::new(),
        })
    }

    pub fn runtime(&self) -> &'runtime Runtime {
        self.runtime
    }

    pub fn owner(&self) -> &'runtime ExecutableModule {
        self.owner
    }

    /// Cooperatively check cancellation during a custom conversion's long loop.
    pub fn poll(&self) -> NativeResult<()> {
        self.runtime.resources().poll_execution()
    }

    pub fn type_for<T: KagariType>(&self) -> NativeResult<TypeArgument> {
        let ty = T::kagari_type(&self.runtime.native_entries.catalog)?;
        self.runtime.native_type_argument(self.owner, &ty)
    }

    pub fn parameter(&self, applied: &TypeArgument, index: usize) -> NativeResult<TypeArgument> {
        applied.validate(self.runtime)?;
        applied.parameter(self.runtime, self.owner, index)
    }

    pub fn encode<T: IntoKagari>(&mut self, value: T) -> NativeResult<RootedValue> {
        let ty = self.type_for::<T>()?;
        let base = self.roots.len();
        let value = self.encode_value(&ty, value)?;
        let rooted = self
            .runtime
            .root_value(value)
            .ok_or_else(|| RuntimeError::module_validation("converted result root"));
        self.roots.truncate(base);
        rooted
    }

    pub fn decode<T: FromKagari>(&mut self, value: &RootedValue) -> NativeResult<T> {
        let ty = self.type_for::<T>()?;
        let value = value
            .value(self.runtime.gc())
            .ok_or_else(|| RuntimeError::module_validation("foreign or expired conversion root"))?;
        self.decode_value(&ty, &value)
    }

    /// Returned raw values remain protected only until the enclosing conversion
    /// scope ends. Public callers normally use `encode` to receive an owning root.
    pub fn encode_value<T: IntoKagari>(
        &mut self,
        expected: &TypeArgument,
        value: T,
    ) -> NativeResult<Value> {
        self.check_type::<T>(expected)?;
        self.encode_prepared(expected, value)
    }

    /// The owning binding has already checked the Rust mapping. Value checks,
    /// roots and conversion limits still apply on every access.
    pub(crate) fn encode_prepared<T: IntoKagari>(
        &mut self,
        expected: &TypeArgument,
        value: T,
    ) -> NativeResult<Value> {
        let (value, root) = self.encode_with(expected, value, |cx, value| cx.protect(&value))?;
        if let Some(root) = root {
            self.roots
                .try_reserve(1)
                .map_err(|_| self.runtime.resources().limit("conversion temporary roots"))?;
            self.roots.push(root);
        }
        Ok(value)
    }

    pub(crate) fn encode_into<T: IntoKagari>(
        &mut self,
        expected: &TypeArgument,
        value: T,
        slots: &mut CallSlots<'_>,
        index: usize,
    ) -> NativeResult<()> {
        if !slots.belongs_to(self.runtime) {
            return Err(RuntimeError::module_validation("foreign argument writer"));
        }
        self.encode_with(expected, value, |_, value| slots.set(index, value))
            .map(|_| ())
    }

    fn encode_with<T: IntoKagari, P>(
        &mut self,
        expected: &TypeArgument,
        value: T,
        publish: impl FnOnce(&Self, Value) -> NativeResult<P>,
    ) -> NativeResult<(Value, P)> {
        let output = {
            let mut frame = self.enter(None)?;
            let value = value
                .into_kagari(&mut frame, expected)
                .map_err(|error| frame.runtime.resources().admit_execution_error(error))?;
            frame.check_value(expected, &value)?;
            let protection = publish(&frame, value)?;
            frame.runtime.gc_safepoint()?;
            (value, protection)
        };
        Ok(output)
    }

    pub fn decode_value<T: FromKagari>(
        &mut self,
        expected: &TypeArgument,
        value: &Value,
    ) -> NativeResult<T> {
        self.check_type::<T>(expected)?;
        self.decode_prepared(expected, value)
    }

    pub(crate) fn decode_prepared<T: FromKagari>(
        &mut self,
        expected: &TypeArgument,
        value: &Value,
    ) -> NativeResult<T> {
        self.check_value(expected, value)?;
        let _root = self.protect(value)?;
        self.decode_protected(expected, value)
    }

    pub(crate) fn decode_result<T: FromKagari>(
        &mut self,
        expected: &TypeArgument,
        slots: &CallSlots<'_>,
    ) -> NativeResult<T> {
        if !slots.belongs_to(self.runtime) {
            return Err(RuntimeError::module_validation("foreign result window"));
        }
        let value = slots.read_result()?;
        self.check_value(expected, &value)?;
        self.decode_protected(expected, &value)
    }

    fn decode_protected<T: FromKagari>(
        &mut self,
        expected: &TypeArgument,
        value: &Value,
    ) -> NativeResult<T> {
        let tracked = if T::PRESERVES_IDENTITY {
            None
        } else {
            identity(value)
        };
        let mut frame = self.enter(tracked)?;
        frame.runtime.gc_safepoint()?;
        let result = T::from_kagari(&mut frame, expected, value);
        result.map_err(|error| frame.runtime.resources().admit_execution_error(error))
    }

    /// The supplying call window already roots this copied value through decoding.
    pub(crate) fn decode_slot<T: FromKagari>(
        &mut self,
        expected: &TypeArgument,
        value: &Value,
    ) -> NativeResult<T> {
        self.check_value(expected, value)?;
        self.decode_protected(expected, value)
    }

    /// Reject an oversized owned container before reserving its output buffer.
    pub fn check_elements(&self, count: usize) -> NativeResult<()> {
        self.poll()?;
        if count > self.limits.max_nodes.saturating_sub(self.nodes) {
            return Err(self.runtime.resources().limit("conversion node limit"));
        }
        Ok(())
    }

    pub fn charge_string(&mut self, bytes: usize) -> NativeResult<()> {
        self.poll()?;
        let next = self
            .string_bytes
            .checked_add(bytes)
            .filter(|bytes| *bytes <= self.limits.max_string_bytes)
            .ok_or_else(|| {
                self.runtime
                    .resources()
                    .limit("conversion string byte limit")
            })?;
        self.string_bytes = next;
        Ok(())
    }

    fn enter(&mut self, identity: Option<HeapObjectId>) -> NativeResult<Frame<'_, 'runtime>> {
        self.poll()?;
        if self.depth >= self.limits.max_depth {
            return Err(self.runtime.resources().limit("conversion depth limit"));
        }
        self.check_elements(1)?;
        if let Some(identity) = identity {
            self.active
                .try_reserve(1)
                .map_err(|_| self.runtime.resources().limit("conversion cycle tracking"))?;
            if !self.active.insert(identity) {
                return Err(RuntimeError::module_validation(
                    "cycle in owned value conversion",
                ));
            }
        }
        self.depth += 1;
        self.nodes += 1;
        let root_base = self.roots.len();
        Ok(Frame {
            context: self,
            root_base,
            identity,
        })
    }

    /// Validate a requested Rust type before executing user code or converters.
    pub fn check_type<T: KagariType>(&self, expected: &TypeArgument) -> NativeResult<()> {
        #[cfg(feature = "execution-diagnostics")]
        diagnostics::record(Event::ConversionTypeCheck);
        expected.validate(self.runtime)?;
        T::check_type(self, expected)
    }

    pub(crate) fn check_declared_type(
        &self,
        expected: &TypeArgument,
        ty: Type,
    ) -> NativeResult<()> {
        expected.validate(self.runtime)?;
        let actual = self.runtime.native_type_argument(self.owner, &ty)?;
        if !expected
            .view(self.owner)
            .compatible(actual.view(self.owner))
        {
            return Err(RuntimeError::module_validation(
                "Rust conversion type differs from its declaration",
            ));
        }
        Ok(())
    }

    pub(crate) fn argument_scope<R>(
        &mut self,
        convert: impl FnOnce(&mut Self) -> NativeResult<R>,
    ) -> NativeResult<R> {
        let mut frame = self.enter(None)?;
        let result = convert(&mut frame);
        result.map_err(|error| frame.runtime.resources().admit_execution_error(error))
    }

    #[cfg(feature = "serde")]
    pub(crate) fn decode_scope<R>(
        &mut self,
        expected: &TypeArgument,
        value: &Value,
        decode: impl FnOnce(&mut Self) -> NativeResult<R>,
    ) -> NativeResult<R> {
        self.check_value(expected, value)?;
        let _root = self.protect(value)?;
        let mut frame = self.enter(identity(value))?;
        frame.runtime.gc_safepoint()?;
        let result = decode(&mut frame);
        result.map_err(|error| frame.runtime.resources().admit_execution_error(error))
    }

    /// Validate a raw boundary value against its checked type and supplying
    /// generation. External references additionally require a live scope and
    /// sufficient access capability; this never dereferences their Rust storage.
    pub fn check_value(&self, expected: &TypeArgument, value: &Value) -> NativeResult<()> {
        #[cfg(feature = "execution-diagnostics")]
        diagnostics::record(Event::ConversionValueCheck);
        expected.validate(self.runtime)?;
        if !expected.matches(self.runtime, value, self.owner) {
            return Err(RuntimeError::module_validation(
                "converted value differs from its declaration",
            ));
        }
        Ok(())
    }

    fn protect(&self, value: &Value) -> NativeResult<Option<RootedValue>> {
        if value.object_id().is_none() {
            return Ok(None);
        }
        self.runtime
            .root_value(*value)
            .map(Some)
            .ok_or_else(|| RuntimeError::module_validation("conversion temporary root"))
    }
}

fn identity(value: &Value) -> Option<HeapObjectId> {
    match value {
        Value::Array(id)
        | Value::Map(id)
        | Value::Set(id)
        | Value::Enum(id)
        | Value::Struct(id)
        | Value::GcHandle(id)
        | Value::Closure(id)
        | Value::Cell(id) => Some(*id),
        Value::Interface(id) => Some(id.0),
        _ => None,
    }
}
