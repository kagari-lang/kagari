//! One synchronous conversion scope protects unpublished values and exact versions.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    gc::{
        reference::GcRef,
        roots::{RootSet, RootedValue},
    },
    module::{ExecutableModule, ModuleEpochRetention, ModuleStore, retention::ProgramLease},
    native::{
        binding::NativeResult,
        context::CallContext,
        conversion::{FromKagari, IntoKagari, KagariType},
        payload::NativeType,
        storage::{NativePayload, value::StoredValue},
        types::{Type, TypeRef},
        value_handle::ScriptValue,
    },
    session::ExecutionRetention,
    value::Value,
};
use kagari_common::identity::DefinitionPath;
use std::{
    collections::HashSet,
    ops::{Deref, DerefMut},
    ptr,
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
    active: HashSet<GcRef>,
    roots: Vec<RootedValue>,
}

/// Exact retained-program authority borrowed from an admitted conversion scope.
/// The context either owns its lease or borrows a checked execution/call scope;
/// neither this token nor a derived context can outlive that protection.
#[derive(Clone, Copy)]
pub(crate) struct ConversionRetention<'scope> {
    context: &'scope ConversionContext<'scope>,
}

impl ConversionRetention<'_> {
    pub(crate) fn covers(&self, runtime: &Runtime, owner: &ExecutableModule) -> NativeResult<bool> {
        self.covers_store(runtime.modules(), owner)
    }

    pub(crate) fn covers_store(
        &self,
        store: &ModuleStore,
        owner: &ExecutableModule,
    ) -> NativeResult<bool> {
        if !ptr::eq(self.context.runtime.modules(), store) {
            return Err(RuntimeError::module_validation(
                "foreign conversion retention",
            ));
        }
        Ok(self.context.owner.program_identity() == owner.program_identity())
    }

    pub(crate) fn retain(
        self,
        runtime: &Runtime,
        owner: &ExecutableModule,
        kind: ModuleEpochRetention,
    ) -> Option<ProgramLease> {
        runtime.resources().ensure_execution_allowed().ok()?;
        runtime.modules().retain_program(owner, kind, Some(self))
    }
}

enum ScopeAuthority<'scope> {
    Execution(&'scope ExecutionRetention<'scope>),
    Conversion(ConversionRetention<'scope>),
}

impl ScopeAuthority<'_> {
    fn covers(&self, runtime: &Runtime, owner: &ExecutableModule) -> NativeResult<bool> {
        match self {
            Self::Execution(authority) => authority.covers(runtime, owner),
            Self::Conversion(authority) => authority.covers(runtime, owner),
        }
    }
}

struct Frame<'scope, 'runtime> {
    context: &'scope mut ConversionContext<'runtime>,
    root_base: usize,
    identity: Option<GcRef>,
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
        Self::prepare(runtime, owner, limits, None)
    }

    /// Convert within an explicitly borrowed execution retention scope.
    ///
    /// Exact runtime/program-generation coverage avoids a second registration.
    /// Uncovered owners retain independently, just like `new`; foreign authority
    /// is rejected. Executable, candidate, installation and execution checks still
    /// run before conversion. The context cannot outlive the lending scope.
    ///
    /// ```compile_fail
    /// use kagari_runtime::{Runtime, module::ExecutableModule};
    /// use kagari_runtime::native::conversion::context::ConversionContext;
    /// fn escape(runtime: &Runtime, owner: &ExecutableModule) {
    ///     let session = runtime.begin_execution(owner, runtime.execution_options()).unwrap();
    ///     let authority = session.retention();
    ///     let context = ConversionContext::in_execution(runtime, owner, &authority).unwrap();
    ///     drop(session);
    ///     drop(context);
    /// }
    /// ```
    pub fn in_execution(
        runtime: &'runtime Runtime,
        owner: &'runtime ExecutableModule,
        authority: &'runtime ExecutionRetention<'_>,
    ) -> NativeResult<Self> {
        Self::prepare(
            runtime,
            owner,
            ConversionLimits::default(),
            Some(ScopeAuthority::Execution(authority)),
        )
    }

    /// Open an independent temporary conversion scope using this exact program's
    /// retention. Other generations retain their own lease; all dynamic admission
    /// and publication checks precede custom conversion code. Limits and scratch
    /// roots start fresh, as with `new`, and the child cannot outlive this context.
    ///
    /// ```compile_fail
    /// use kagari_runtime::{Runtime, module::ExecutableModule};
    /// use kagari_runtime::native::conversion::context::ConversionContext;
    /// fn escape<'a>(runtime: &'a Runtime, owner: &'a ExecutableModule)
    ///     -> ConversionContext<'a>
    /// {
    ///     let parent = ConversionContext::new(runtime, owner).unwrap();
    ///     parent.scoped(owner).unwrap()
    /// }
    /// ```
    pub fn scoped<'scope>(
        &'scope self,
        owner: &'scope ExecutableModule,
    ) -> NativeResult<ConversionContext<'scope>> {
        ConversionContext::prepare(
            self.runtime,
            owner,
            ConversionLimits::default(),
            Some(ScopeAuthority::Conversion(self.retention())),
        )
    }

    pub(crate) fn retention(&self) -> ConversionRetention<'_> {
        ConversionRetention { context: self }
    }

    fn prepare(
        runtime: &'runtime Runtime,
        owner: &'runtime ExecutableModule,
        limits: ConversionLimits,
        authority: Option<ScopeAuthority<'runtime>>,
    ) -> NativeResult<Self> {
        runtime.gc().ensure_no_storage_borrow()?;
        runtime.resources().ensure_execution_allowed()?;
        runtime.validate_executable_module(owner)?;
        let covered = authority
            .map(|authority| authority.covers(runtime, owner))
            .transpose()?
            .unwrap_or(false);
        let program = if covered {
            // Borrowed retention replaces the lease/member walk, not publication
            // admission: diagnostic availability alone permits a faulted graph.
            runtime.modules().publish_program(owner)?;
            None
        } else {
            Some(
                runtime
                    .retain_program(owner, ModuleEpochRetention::ActiveCall)
                    .ok_or_else(|| {
                        RuntimeError::module_validation("conversion program retention")
                    })?,
            )
        };
        Ok(Self {
            runtime,
            owner,
            _program: program,
            limits,
            depth: 0,
            nodes: 0,
            string_bytes: 0,
            active: HashSet::new(),
            roots: Vec::new(),
        })
    }

    /// The synchronous call already retains its exact program through the
    /// execution frame or selected-call lease. Only escaping handles add leases.
    pub(crate) fn in_native_call(call: &'runtime CallContext<'_>) -> NativeResult<Self> {
        call.heap().ensure_no_storage_borrow()?;
        call.runtime.resources().ensure_execution_allowed()?;
        // CallContext is constructed only from an authenticated execution frame
        // or a checked selected-call lease. That owner remains pinned throughout
        // this borrow; revalidating installation/liveness would repeat admission.
        // Dynamic execution and storage exclusion checks above still apply.
        Ok(Self {
            runtime: call.runtime,
            owner: call.owner,
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

    /// Resolve a declaration from this runtime's installed native catalog.
    pub fn type_reference(&self, declaration: &DefinitionPath) -> NativeResult<TypeRef> {
        self.runtime
            .native_entries
            .catalog
            .type_reference(declaration)
    }

    /// Apply a registered opaque declaration while preserving each argument's scope.
    pub fn apply_type(
        &self,
        declaration: &TypeRef,
        arguments: &[TypeArgument],
    ) -> NativeResult<TypeArgument> {
        self.runtime.apply_native_type(
            self.owner,
            declaration.id(),
            declaration.definitions(),
            arguments,
        )
    }

    /// Prepare checked provider admission for an exact opaque application.
    pub fn native_type<T: NativePayload>(
        &self,
        argument: &TypeArgument,
    ) -> NativeResult<NativeType<T>> {
        self.runtime
            .prepare_native_type(self.owner, argument.clone(), Some(self.retention()))
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
            .root_value(value.value(self.runtime)?)
            .ok_or_else(|| RuntimeError::module_validation("converted result root"));
        self.roots.truncate(base);
        rooted
    }

    pub fn decode<T: FromKagari>(&mut self, value: &RootedValue) -> NativeResult<T> {
        let ty = self.type_for::<T>()?;
        self.check_type::<T>(&ty)?;
        let value = value
            .value(self.runtime.gc())
            .ok_or_else(|| RuntimeError::module_validation("foreign or expired conversion root"))?;
        self.decode_prepared(&ty, &value)
    }

    /// Attach an exact declared type to an owning result without copying its graph.
    /// Rejects foreign/expired roots and mismatched types. The existing root lease
    /// and its alias behavior are preserved; no additional root is registered.
    pub fn retain(
        &mut self,
        expected: &TypeArgument,
        root: RootedValue,
    ) -> NativeResult<ScriptValue> {
        self.runtime.gc().ensure_execution_allowed()?;
        let value = root
            .value(self.runtime.gc())
            .ok_or_else(|| RuntimeError::module_validation("foreign or expired conversion root"))?;
        self.check_value(expected, &value)?;
        Ok(ScriptValue::from_root(
            root,
            expected.clone(),
            self.owner.clone(),
        ))
    }

    /// Retain a checked slot independently of its source pack and later replacements.
    /// Rejects foreign/expired packs, absent slots and mismatched declarations.
    /// Managed values acquire independent retention; scalars need no root entry.
    pub fn retain_slot(
        &mut self,
        expected: &TypeArgument,
        roots: &RootSet,
        index: usize,
    ) -> NativeResult<ScriptValue> {
        let value = roots.get(self.runtime.gc(), index).ok_or_else(|| {
            RuntimeError::module_validation("foreign, expired or missing conversion slot")
        })?;
        self.retain_converted(expected, value)
    }

    /// Retain a built-in converter's result before returning to caller code.
    /// This callback-free transition stays private; external converters compose
    /// owning conversions instead of importing freely copied managed transport.
    pub(crate) fn retain_converted(
        &mut self,
        expected: &TypeArgument,
        value: Value,
    ) -> NativeResult<ScriptValue> {
        ScriptValue::from_transport(self, expected, &value)
    }

    /// Encode against an explicit declared type and return independent retention.
    /// Managed results remain live after this context and its scratch roots end.
    pub fn encode_value<T: IntoKagari>(
        &mut self,
        expected: &TypeArgument,
        value: T,
    ) -> NativeResult<ScriptValue> {
        self.check_type::<T>(expected)?;
        self.encode_retained_prepared(expected, value)
    }

    /// The owning binding has already checked the Rust mapping. Value checks,
    /// roots and conversion limits still apply on every access.
    pub(crate) fn encode_prepared<T: IntoKagari>(
        &mut self,
        expected: &TypeArgument,
        value: T,
    ) -> NativeResult<Value> {
        self.encode_retained_prepared(expected, value)?
            .value(self.runtime)
    }

    /// Internal transport stays protected by this context until publication.
    pub(crate) fn encode_transport<T: IntoKagari>(
        &mut self,
        expected: &TypeArgument,
        value: T,
    ) -> NativeResult<Value> {
        self.encode_value(expected, value)?.value(self.runtime)
    }

    fn encode_retained_prepared<T: IntoKagari>(
        &mut self,
        expected: &TypeArgument,
        value: T,
    ) -> NativeResult<ScriptValue> {
        let (value, root) = {
            let mut frame = self.enter(None)?;
            let converted = value
                .into_kagari(&mut frame, expected)
                .map_err(|error| frame.runtime.resources().admit_execution_error(error))?;
            let value = converted.value(frame.runtime)?;
            frame.check_value(expected, &value)?;
            let root = converted.retained_root().cloned();
            frame.runtime.gc_safepoint()?;
            (converted, root)
        };
        if let Some(root) = root {
            self.roots
                .try_reserve(1)
                .map_err(|_| self.runtime.resources().limit("conversion temporary roots"))?;
            self.roots.push(root);
        }
        Ok(value)
    }

    /// Decode a retained value against an exact declared type and access view.
    /// Custom type checks run before snapshotting the input; conversion then owns
    /// independent protection across callbacks, collection and source replacement.
    ///
    /// ```compile_fail
    /// use kagari_runtime::{
    ///     frame::types::arguments::TypeArgument,
    ///     native::{conversion::context::ConversionContext, value_handle::ScriptValue},
    ///     value::Value,
    /// };
    /// fn unprotected(cx: &mut ConversionContext<'_>, ty: &TypeArgument, raw: Value) {
    ///     let _: ScriptValue = cx.decode_value(ty, &raw).unwrap();
    /// }
    /// ```
    pub fn decode_value<T: FromKagari>(
        &mut self,
        expected: &TypeArgument,
        value: &ScriptValue,
    ) -> NativeResult<T> {
        self.check_type::<T>(expected)?;
        let raw = value.call_argument(self.runtime, expected, self.owner)?;
        self.decode_prepared(expected, &raw)
    }

    /// Decode backend transport whose storage is protected by its caller.
    /// Ordinary host adapters use `decode_value` with a retained handle.
    /// Type, owner, conversion limits and result validation remain checked.
    ///
    /// # Safety
    /// Every managed value must remain live in its originating runtime until
    /// this call establishes independent retention. The caller must own an
    /// immutable rooted snapshot or otherwise prevent collection and replacement
    /// of the source before admission. Raw transport may not be recovered from
    /// an expired root. This obligation applies even when conversion returns an
    /// error; dynamic type checks do not make dangling transport safe.
    #[doc(hidden)]
    pub unsafe fn decode_transport<T: FromKagari>(
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
        let protected = self.retain_converted(expected, *value)?;
        let tracked = if T::PRESERVES_IDENTITY {
            None
        } else {
            identity(value)
        };
        let mut frame = self.enter(tracked)?;
        frame.runtime.gc_safepoint()?;
        let result = T::from_kagari(&mut frame, expected, &protected);
        result.map_err(|error| frame.runtime.resources().admit_execution_error(error))
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

    fn enter(&mut self, identity: Option<GcRef>) -> NativeResult<Frame<'_, 'runtime>> {
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

    /// Run conversions with temporary roots released at the end of this scope.
    /// Returned values must own independent roots or be scalar; raw GC values
    /// cannot escape unless the caller has retained them separately.
    pub fn argument_scope<R>(
        &mut self,
        convert: impl FnOnce(&mut Self) -> NativeResult<R>,
    ) -> NativeResult<R> {
        let mut frame = self.enter(None)?;
        let result = convert(&mut frame);
        result.map_err(|error| frame.runtime.resources().admit_execution_error(error))
    }

    /// Validate and root an input during recursive schema decoding, enforcing
    /// conversion depth, cycle detection, cancellation, and temporary-root cleanup.
    #[cfg(feature = "serde")]
    pub fn decode_scope<R>(
        &mut self,
        expected: &TypeArgument,
        value: &ScriptValue,
        decode: impl FnOnce(&mut Self, &ScriptValue) -> NativeResult<R>,
    ) -> NativeResult<R> {
        let raw = value.call_argument(self.runtime, expected, self.owner)?;
        self.check_value(expected, &raw)?;
        let protected = self.retain_converted(expected, raw)?;
        let mut frame = self.enter(identity(&raw))?;
        frame.runtime.gc_safepoint()?;
        let result = decode(&mut frame, &protected);
        result.map_err(|error| frame.runtime.resources().admit_execution_error(error))
    }

    /// Validate protected backend transport using the ordinary type/access checks.
    ///
    /// # Safety
    /// The caller must keep the input live in its originating runtime through this
    /// check. Scoped capabilities require their original active host scope. This
    /// does not retain the value or turn expired transport into a live input.
    #[doc(hidden)]
    pub unsafe fn check_transport(
        &self,
        expected: &TypeArgument,
        value: &Value,
    ) -> NativeResult<()> {
        self.check_value(expected, value)
    }

    /// Import protected transport into a scanned Native payload after exact type
    /// and heap admission. This does not retain the value independently.
    ///
    /// # Safety
    /// Keep the source traced through validation and publication, and install the
    /// returned carrier before source mutation, expiry or a collecting callback.
    #[doc(hidden)]
    pub unsafe fn store_transport(
        &self,
        expected: &TypeArgument,
        value: Value,
    ) -> NativeResult<StoredValue> {
        self.check_value(expected, &value)?;
        if !value.is_default_heap_payload(self.runtime.gc()) {
            return Err(RuntimeError::module_validation("native stored transport"));
        }
        self.runtime.gc().publish_value(value)?;
        Ok(StoredValue::new(value))
    }

    /// Validate a raw boundary value against its checked type and supplying
    /// generation. External references additionally require a live scope and
    /// sufficient access capability; this never dereferences their Rust storage.
    pub(crate) fn check_value(&self, expected: &TypeArgument, value: &Value) -> NativeResult<()> {
        expected.validate(self.runtime)?;
        if !expected.matches(self.runtime, value, self.owner) {
            return Err(RuntimeError::module_validation(
                "converted value differs from its declaration",
            ));
        }
        Ok(())
    }
}

fn identity(value: &Value) -> Option<GcRef> {
    match value {
        Value::Array(id) | Value::GcHandle(id) => Some(*id),
        Value::Enum(reference) | Value::Closure(reference) | Value::Cell(reference) => {
            Some(*reference)
        }
        Value::Interface(id) => Some(id.0),
        Value::Struct(reference) => Some(*reference),
        _ => None,
    }
}
