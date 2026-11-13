//! One synchronous conversion scope owns its protected execution-local suffix.
mod aggregates;
mod arguments;
mod arrays;
mod closures;
mod enumerations;
mod futures;
pub(crate) mod inputs;
mod interfaces;
mod methods;
pub(crate) mod native;
mod ranges;
mod results;
pub(crate) mod structures;
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};
use crate::{
    Runtime,
    error::{RuntimeError, fault::EngineFault},
    execution::roots::{ExecutionRoots, LocalMark, SessionId, Slots, ValueBank},
    execution_metadata::managed::sources::MetadataSource,
    frame::types::arguments::TypeArgument,
    module::{ExecutableModule, managed::source::RevisionSource},
    native::{
        binding::NativeResult,
        context::NativeCall,
        conversion::{FromKagari, IntoKagari, KagariType},
        types::Type,
    },
    publication::ActivationMark,
    value::{
        EphemeralValue,
        carriers::{RootedValue, ScopedValue},
        host_values::EphemeralObject,
        kind::ObjectKind,
        strings::StringObject,
        view::ValueView,
    },
    value_check,
};
use kagari_gc::{error::GcError, heap::Phase, object::Gc};
use std::{
    marker::PhantomData,
    ops::{Deref, DerefMut},
    rc::Rc,
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

/// Borrow the owning runtime only for synchronous conversion. Encoded views name
/// this owner's protected source suffix; publish them before it retires or use
/// explicit retain for Host ownership. Scratch slots expire on every exit.
pub struct ConversionContext<'runtime> {
    activation: Option<ActivationMark>,
    _thread: PhantomData<Rc<()>>,
    runtime: &'runtime mut Runtime,
    owner: &'runtime ExecutableModule,
    execution: Option<SessionId>,
    origin: RevisionSource<'runtime>,
    metadata: Option<MetadataSource<'runtime>>,
    owns_execution: bool,
    locals: Option<LocalMark>,
    limits: ConversionLimits,
    depth: usize,
    nodes: usize,
    string_bytes: usize,
    active: Vec<ValueView<'runtime>>,
}

enum Admission<'origin> {
    Host,
    Mapping,
    Native {
        execution: SessionId,
        origin: RevisionSource<'origin>,
        metadata: Option<MetadataSource<'origin>>,
    },
}

/// Native conversion and streaming Serde adapters share this suffix owner.
/// It admits no execution and exposes no independently owned value storage.
pub(in crate::native) struct Frame<'scope, 'runtime> {
    context: &'scope mut ConversionContext<'runtime>,
    locals: Option<LocalMark>,
    counted: bool,
    tracked: bool,
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

impl<'runtime> Frame<'_, 'runtime> {
    pub(in crate::native) fn preserve(
        &mut self,
        value: ValueView<'runtime>,
    ) -> NativeResult<ValueView<'runtime>> {
        let mark = self.locals.as_mut().expect("conversion frame mark");
        let result = preserve_value(self.context.runtime, mark, value);
        result.map_err(|error| self.context.gc_error(error))
    }
}

/// Both child conversion frames and the final Native handoff use this same
/// physical compaction. Validation precedes any growth or old suffix retirement.
pub(in crate::native) fn preserve_value<'value>(
    runtime: &mut Runtime,
    mark: &mut LocalMark,
    value: ValueView<'value>,
) -> Result<ValueView<'value>, GcError> {
    let Some(location) = value.location() else {
        return Ok(value);
    };
    runtime.memory.scope(&runtime.resources, |cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            let scoped = value.get(slots)?;
            if location.execution != mark.execution()
                || !matches!(location.bank, ValueBank::Local)
                || location.slot < mark.top()
            {
                return Ok(value);
            }
            let location = slots.preserve_local(mark, scoped)?;
            ValueView::protected(scoped, Some(location))
        })
    })
}

impl Drop for Frame<'_, '_> {
    fn drop(&mut self) {
        if let Some(mark) = self.locals.take() {
            self.context.runtime.retire_execution_locals(mark);
        }
        if self.counted {
            self.context.depth -= 1;
        }
        if self.tracked {
            self.context.active.pop();
        }
    }
}

impl<'runtime> ConversionContext<'runtime> {
    /// Borrow a matching admitted body for Host conversion, including candidate
    /// initialization; otherwise require its current publication. This scope owns
    /// temporary source cells, while explicit retain creates escaping Host roots.
    pub fn new(
        runtime: &'runtime mut Runtime,
        owner: &'runtime ExecutableModule,
    ) -> NativeResult<Self> {
        Self::with_limits(runtime, owner, ConversionLimits::default())
    }

    pub fn with_limits(
        runtime: &'runtime mut Runtime,
        owner: &'runtime ExecutableModule,
        limits: ConversionLimits,
    ) -> NativeResult<Self> {
        Self::prepare(runtime, owner, limits, Admission::Host)
    }

    pub(crate) fn for_mapping(
        runtime: &'runtime mut Runtime,
        owner: &'runtime ExecutableModule,
    ) -> NativeResult<Self> {
        Self::prepare(
            runtime,
            owner,
            ConversionLimits::default(),
            Admission::Mapping,
        )
    }

    fn prepare(
        runtime: &'runtime mut Runtime,
        owner: &'runtime ExecutableModule,
        limits: ConversionLimits,
        admission: Admission<'runtime>,
    ) -> NativeResult<Self> {
        let resources = &runtime.resources;
        let activation = match admission {
            Admission::Host => Some(resources.begin_synchronous_activation()?),
            Admission::Mapping | Admission::Native { .. } => None,
        };
        resources.ensure_execution_allowed()?;
        if !matches!(admission, Admission::Native { .. }) {
            runtime.validate_executable_module(owner)?;
        }
        // Explicit Host access inside an admitted body borrows that actual
        // source, including an unpublished candidate. Native sources remain exact.
        let origin = match admission {
            Admission::Native { origin, .. } => origin,
            Admission::Host | Admission::Mapping => runtime.selection_origin(owner)?,
        };
        let (execution, owns_execution) = match admission {
            Admission::Mapping => (None, false),
            Admission::Native { execution, .. } => (Some(execution), false),
            Admission::Host => match resources.active_session().map(|state| state.id) {
                Some(id) => (Some(id), false),
                None => (
                    Some(
                        runtime
                            .memory
                            .enter_execution(&runtime.resources)
                            .map_err(|error| runtime.managed_memory_error(error))?,
                    ),
                    true,
                ),
            },
        };
        let locals = match execution {
            Some(id) => match runtime.memory.scope(&runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| slots.mark_locals(id))
            }) {
                Ok(mark) => Some(mark),
                Err(error) => {
                    let error = runtime.managed_memory_error(error);
                    if owns_execution
                        && let Err(cleanup) =
                            runtime.memory.retire_execution(&runtime.resources, id)
                        && !runtime.resources().is_quarantined()
                    {
                        return Err(runtime
                            .resources()
                            .quarantine(EngineFault::ExecutionStorageRetirementFailed(cleanup)));
                    }
                    return Err(error);
                }
            },
            None => None,
        };
        Ok(Self {
            activation: activation.map(|mark| mark.detach()),
            _thread: Default::default(),
            runtime,
            owner,
            execution,
            origin,
            metadata: match admission {
                Admission::Native { metadata, .. } => metadata,
                Admission::Host | Admission::Mapping => None,
            },
            owns_execution,
            locals,
            limits,
            depth: 0,
            nodes: 0,
            string_bytes: 0,
            active: Vec::new(),
        })
    }

    pub(crate) fn in_native_call(call: &'runtime mut NativeCall<'_>) -> NativeResult<Self> {
        let owner = call.owner;
        Self::for_native_target(call, owner)
    }

    /// A selected callback has its own lexical type owner while borrowing the
    /// enclosing Native execution. No Host activation or root pack is created.
    pub(in crate::native) fn for_native_target(
        call: &'runtime mut NativeCall<'_>,
        owner: &'runtime ExecutableModule,
    ) -> NativeResult<Self> {
        let origin = if owner.program_identity() == call.owner.program_identity() {
            RevisionSource::Execution(call.pending.body())
        } else {
            call.runtime.selection_origin(owner)?
        };
        Self::prepare(
            call.runtime,
            owner,
            ConversionLimits::default(),
            Admission::Native {
                execution: call.pending.execution(),
                origin,
                metadata: call.pending.application().map(MetadataSource::Application),
            },
        )
    }

    pub fn runtime(&self) -> &Runtime {
        self.runtime
    }

    pub fn runtime_mut(&mut self) -> &mut Runtime {
        self.runtime
    }

    pub fn owner(&self) -> &'runtime ExecutableModule {
        self.owner
    }

    /// The constructor selected this lexical owner explicitly: an admitted
    /// Native body borrows its source; Host/current-target preparation reopens
    /// current publication. This never searches an ID table for retired state.
    pub(in crate::native) fn owner_origin(&self) -> RevisionSource<'runtime> {
        self.origin
    }

    pub(in crate::native) fn owner_metadata(&self) -> Option<MetadataSource<'runtime>> {
        self.metadata
    }

    /// Reborrow the execution for an already selected callback. The parent
    /// conversion owns preparation; this child owns only its call/result suffix.
    pub(crate) fn for_selected(
        runtime: &'runtime mut Runtime,
        owner: &'runtime ExecutableModule,
        execution: SessionId,
        origin: RevisionSource<'runtime>,
        metadata: Option<MetadataSource<'runtime>>,
    ) -> NativeResult<Self> {
        Self::prepare(
            runtime,
            owner,
            ConversionLimits::default(),
            Admission::Native {
                execution,
                origin,
                metadata,
            },
        )
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
        let mut scratch = self.scratch()?;
        let value = scratch.encode_value(&ty, value)?;
        scratch.retain(&value)
    }

    pub fn decode<T: FromKagari>(&mut self, value: &RootedValue) -> NativeResult<T> {
        let ty = self.type_for::<T>()?;
        let mut scratch = self.scratch()?;
        let value = scratch.import(value)?;
        scratch.decode_value(&ty, &value)
    }

    /// Return a call-lifetime view. Managed scratch stays in the complete source;
    /// only encode/retain creates a genuine independently owning Host handle.
    pub fn encode_value<T: IntoKagari>(
        &mut self,
        expected: &TypeArgument,
        value: T,
    ) -> NativeResult<ValueView<'runtime>> {
        self.check_type::<T>(expected)?;
        self.encode_prepared(expected, value)
    }

    pub(crate) fn encode_prepared<T: IntoKagari>(
        &mut self,
        expected: &TypeArgument,
        value: T,
    ) -> NativeResult<ValueView<'runtime>> {
        let mut frame = self.enter(None)?;
        let value = value
            .into_kagari(&mut frame, expected)
            .map_err(|error| frame.runtime.resources().admit_execution_error(error))?;
        frame.check_value(expected, &value)?;
        frame.runtime.gc_safepoint()?;
        frame.preserve(value)
    }

    pub fn decode_value<T: FromKagari>(
        &mut self,
        expected: &TypeArgument,
        value: &ValueView<'_>,
    ) -> NativeResult<T> {
        self.check_type::<T>(expected)?;
        self.decode_prepared(expected, value)
    }

    pub(crate) fn decode_prepared<T: FromKagari>(
        &mut self,
        expected: &TypeArgument,
        value: &ValueView<'_>,
    ) -> NativeResult<T> {
        self.check_value(expected, value)?;
        self.decode_admitted(expected, value)
    }

    /// The complete argument owner checked every value before the first custom
    /// converter. Reuse that admission while its immutable source prefix lives.
    pub(crate) fn decode_admitted<T: FromKagari>(
        &mut self,
        expected: &TypeArgument,
        value: &ValueView<'_>,
    ) -> NativeResult<T> {
        let identity =
            (!T::PRESERVES_IDENTITY && identity_kind(value.kind())).then_some(value.reborrow());
        let mut frame = self.enter(identity)?;
        frame.runtime.gc_safepoint()?;
        T::from_kagari(&mut frame, expected, value)
            .map_err(|error| frame.runtime.resources().admit_execution_error(error))
    }

    /// Import a genuine Host root into this conversion's protected local area.
    pub fn import(&mut self, root: &RootedValue) -> NativeResult<ValueView<'runtime>> {
        value_check::candidate_root(self.runtime, root)?;
        let execution = self.execution_id()?;
        self.with_values(|slots| {
            let value = root.get(slots.scope())?;
            let location = if value.object().is_some() {
                Some(slots.push_local(execution, value)?)
            } else {
                None
            };
            ValueView::protected(value, location)
        })
    }

    pub fn retain(&mut self, value: &ValueView<'_>) -> NativeResult<RootedValue> {
        value_check::candidate_protected(self.runtime, &[*value])?;
        self.with_values(|slots| value.get(slots)?.retain(slots.scope()))
    }

    pub fn allocate_string(&mut self, text: String) -> NativeResult<ValueView<'runtime>> {
        let execution = self.execution_id()?;
        self.with_values(|slots| {
            let object = StringObject::alloc(slots.scope(), text)?;
            let value = ScopedValue::managed(object);
            slots.protect_local(execution, value)
        })
    }

    /// Publish an inert scoped Host capability into this conversion's suffix.
    /// The Host loan still controls access and expires independently of GC.
    pub(crate) fn allocate_ephemeral(
        &mut self,
        capability: EphemeralValue,
    ) -> NativeResult<ValueView<'runtime>> {
        let execution = self.execution_id()?;
        self.with_values(|slots| {
            let value = EphemeralObject::alloc(slots.scope(), capability)?;
            let location = if value.object().is_some() {
                Some(slots.push_local(execution, value)?)
            } else {
                None
            };
            ValueView::protected(value, location)
        })
    }

    pub fn string_len(&mut self, value: &ValueView<'_>) -> NativeResult<usize> {
        self.read_string(value, str::len)
    }

    pub fn read_string<R>(
        &mut self,
        value: &ValueView<'_>,
        read: impl for<'s> FnOnce(&'s str) -> R,
    ) -> NativeResult<R> {
        self.with_values(|slots| {
            let value = value.get(slots)?;
            StringObject::read(slots.scope(), value, read)
        })
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

    fn enter(
        &mut self,
        identity: Option<ValueView<'runtime>>,
    ) -> NativeResult<Frame<'_, 'runtime>> {
        self.poll()?;
        if self.depth >= self.limits.max_depth {
            return Err(self.runtime.resources().limit("conversion depth limit"));
        }
        self.check_elements(1)?;
        if let Some(value) = identity {
            if self.has_active_ancestor(value)? {
                return Err(RuntimeError::module_validation(
                    "cycle in owned value conversion",
                ));
            }
            self.active
                .try_reserve(1)
                .map_err(|_| self.runtime.resources().limit("conversion cycle tracking"))?;
        }
        let mut frame = self.scratch()?;
        if let Some(value) = identity {
            frame.active.push(value);
            frame.tracked = true;
        }
        frame.depth += 1;
        frame.nodes += 1;
        frame.counted = true;
        Ok(frame)
    }

    fn has_active_ancestor(&mut self, value: ValueView<'_>) -> NativeResult<bool> {
        let active = &self.active;
        if active.is_empty() {
            return Ok(false);
        }
        self.runtime.resources().ensure_execution_allowed()?;
        let result = self
            .runtime
            .memory
            .scope(&self.runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| {
                    let value = value.get(slots)?.object().ok_or(GcError::Representation)?;
                    for ancestor in active {
                        let ancestor = ancestor
                            .get(slots)?
                            .object()
                            .ok_or(GcError::Representation)?;
                        if Gc::ptr_eq(value, ancestor) {
                            return Ok(true);
                        }
                    }
                    Ok(false)
                })
            });
        result.map_err(|error| self.gc_error(error))
    }

    pub(in crate::native) fn scratch(&mut self) -> NativeResult<Frame<'_, 'runtime>> {
        let execution = self.execution_id()?;
        let locals = self.with_values(|slots| slots.mark_locals(execution))?;
        Ok(Frame {
            context: self,
            locals: Some(locals),
            counted: false,
            tracked: false,
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
        value: &ValueView<'_>,
        decode: impl FnOnce(&mut Self) -> NativeResult<R>,
    ) -> NativeResult<R> {
        self.check_value(expected, value)?;
        let identity = identity_kind(value.kind()).then_some(value.reborrow());
        let mut frame = self.enter(identity)?;
        frame.runtime.gc_safepoint()?;
        let result = decode(&mut frame);
        result.map_err(|error| frame.runtime.resources().admit_execution_error(error))
    }

    /// Check current value/access facts against the prepared lexical contract.
    pub fn check_value(
        &mut self,
        expected: &TypeArgument,
        value: &ValueView<'_>,
    ) -> NativeResult<()> {
        value_check::check_protected(self.runtime, self.owner, expected, value)
    }

    /// Keep a logical mismatch distinct from failed admission. Native receiver
    /// adaptation may accept another representation, never a failed capability,
    /// type-generation, resource or collector check.
    pub(in crate::native) fn matches_value(
        &mut self,
        expected: &TypeArgument,
        value: &ValueView<'_>,
    ) -> NativeResult<bool> {
        value_check::matches_protected(self.runtime, self.owner, expected, value)
    }

    pub(in crate::native) fn execution_id(&self) -> NativeResult<SessionId> {
        self.execution.ok_or_else(|| {
            RuntimeError::module_validation("mapping context cannot access execution values")
        })
    }

    fn with_values<T>(
        &mut self,
        operation: impl for<'gc> FnOnce(&mut Slots<'_, 'gc>) -> Result<T, GcError>,
    ) -> NativeResult<T> {
        self.runtime.resources().ensure_execution_allowed()?;
        self.runtime
            .memory
            .scope(&self.runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, operation)
            })
            .map_err(|error| self.gc_error(error))
    }

    fn gc_error(&self, error: GcError) -> RuntimeError {
        match error {
            GcError::ForeignHeap
            | GcError::InvalidRoot
            | GcError::Representation
            | GcError::Index => RuntimeError::module_validation(error.to_string()),
            error => self.runtime.managed_memory_error(error),
        }
    }
}

impl Drop for ConversionContext<'_> {
    fn drop(&mut self) {
        if let Some(mark) = self.locals.take() {
            self.runtime.retire_execution_locals(mark);
        }
        if self.owns_execution
            && !matches!(
                self.runtime.memory.stats().phase,
                Phase::Closed | Phase::Faulted
            )
            && let Some(id) = self.execution
            && let Err(error) = self
                .runtime
                .memory
                .retire_execution(&self.runtime.resources, id)
            && !self.runtime.resources().is_quarantined()
        {
            self.runtime
                .resources()
                .quarantine(EngineFault::ExecutionStorageRetirementFailed(error));
        }
        if let Some(mark) = self.activation.take() {
            mark.retire(self.runtime.resources());
        }
    }
}

fn identity_kind(kind: Option<ObjectKind>) -> bool {
    matches!(
        kind,
        Some(
            ObjectKind::Array
                | ObjectKind::Enum
                | ObjectKind::Struct
                | ObjectKind::Native
                | ObjectKind::Closure
                | ObjectKind::Cell
                | ObjectKind::Interface
        )
    )
}
