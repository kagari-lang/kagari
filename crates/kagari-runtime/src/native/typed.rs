//! Concrete Rust callbacks share the checked conversion boundary with host access.
mod loan_arguments;
pub mod loans;
mod payload;
mod receiver;
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    gc::GcCollection,
    module::ExecutableModule,
    native::{
        binding::{Codec, LinkedNativeFunction, NativeBinding, NativeResult},
        context::{CallContext, LinkedCallable, LinkedOperation, ScriptInvoker},
        conversion::{
            FromKagari, IntoKagari, arguments::FromKagariArguments, context::ConversionContext,
        },
        typed::receiver::ReceiverArguments,
    },
    value::Value,
};
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_types::declaration::NativeDeclaration;

/// One synchronous native invocation. Arguments are owned Rust data or retained
/// handles; no argument-slot or payload borrow spans the callback.
pub struct NativeContext<'call> {
    pub(crate) conversion: ConversionContext<'call>,
    pub(crate) invoke_script: Option<ScriptInvoker>,
    pub(crate) function: Option<&'call LinkedNativeFunction>,
}

impl<'call> NativeContext<'call> {
    /// Open a synchronous host access scope for a retained program version.
    pub fn new(runtime: &'call Runtime, owner: &'call ExecutableModule) -> NativeResult<Self> {
        Ok(Self {
            conversion: ConversionContext::new(runtime, owner)?,
            invoke_script: None,
            function: None,
        })
    }

    /// Backend integration: install synchronous execution for typed calls.
    /// Native callbacks inherit this service from their invoking backend.
    pub fn with_invoker(
        runtime: &'call Runtime,
        owner: &'call ExecutableModule,
        invoke_script: ScriptInvoker,
    ) -> NativeResult<Self> {
        Ok(Self {
            conversion: ConversionContext::new(runtime, owner)?,
            invoke_script: Some(invoke_script),
            function: None,
        })
    }

    pub fn runtime(&self) -> &'call Runtime {
        self.conversion.runtime()
    }

    /// The exact publication supplying this synchronous conversion scope.
    pub fn owner(&self) -> &ExecutableModule {
        self.conversion.owner()
    }

    /// Checked conversions share the invocation's retained owner and temporary roots.
    pub fn conversion(&mut self) -> &mut ConversionContext<'call> {
        &mut self.conversion
    }

    /// Lend exact program retention to an independent temporary conversion. The
    /// child cannot outlive this context; other program generations retain their
    /// own lease. No store borrow or payload loan crosses conversion or reentry.
    pub fn conversion_scope<'scope>(
        &'scope self,
        owner: &'scope ExecutableModule,
    ) -> NativeResult<ConversionContext<'scope>> {
        self.conversion.scoped(owner)
    }

    /// Retain this native invocation's complete result contract.
    pub fn result_type_argument(&self) -> NativeResult<TypeArgument> {
        self.function
            .ok_or_else(|| RuntimeError::module_validation("native result outside invocation"))?
            .type_signature()
            .map(|signature| signature.result.clone())
    }

    /// Inspect immutable callable evidence supplied by this checked invocation.
    /// This lends signature/intrinsic facts; it grants no unrooted call authority.
    pub fn selected_at(&self, slot: usize) -> NativeResult<&LinkedCallable> {
        self.function
            .and_then(|function| function.selected.get(slot))
            .and_then(LinkedOperation::ready)
            .ok_or_else(|| RuntimeError::module_validation("native selected callable slot"))
    }

    pub fn poll(&self) -> NativeResult<()> {
        self.conversion.poll()
    }

    pub fn collect_garbage(&self) -> NativeResult<GcCollection> {
        self.runtime().collect_garbage()
    }

    /// Collect owned Rust values with cooperative cancellation and checked
    /// capacity growth. An iterator's individual `next` call is not preemptible.
    pub fn collect<T>(&self, values: impl IntoIterator<Item = T>) -> NativeResult<Vec<T>> {
        let mut result = Vec::new();
        let mut values = values.into_iter();
        loop {
            self.poll()?;
            let Some(value) = values.next() else { break };
            self.poll()?;
            result
                .try_reserve(1)
                .map_err(|_| RuntimeError::resource_limit("native collected values"))?;
            result.push(value);
        }
        Ok(result)
    }
}

impl NativeBinding {
    /// Attach a typed callback to a preauthored declaration, including generic
    /// signatures. Module installation still validates the complete declaration
    /// and binding; every invocation checks concrete Rust mappings before effects.
    /// Ordinary authored registrations should use ModuleBuilder::bind.
    pub fn declared<A: FromKagariArguments, R: IntoKagari>(
        declaration: &DefinitionMetadata<NativeDeclaration>,
        entry: impl for<'call> Fn(&mut NativeContext<'call>, A) -> NativeResult<R>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        let mut binding = Self::contextual(declaration.records().function.params.len(), entry);
        binding.requirement_owner = Some((
            declaration.records().declaration,
            declaration.definitions().clone(),
        ));
        binding
    }

    pub(crate) fn contextual_method<S: FromKagari, A: FromKagariArguments, R: IntoKagari>(
        arity: usize,
        entry: impl for<'call> Fn(&mut NativeContext<'call>, S, A) -> NativeResult<R>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self::contextual(arity, move |cx, ReceiverArguments(receiver, arguments)| {
            entry(cx, receiver, arguments)
        })
    }

    /// Contextual mappings inherit the checked declaration at invocation. Only
    /// declaration builders supply the arity; all Rust types are checked before
    /// argument conversion or callback effects.
    pub(crate) fn contextual<A: FromKagariArguments, R: IntoKagari>(
        arity: usize,
        entry: impl for<'call> Fn(&mut NativeContext<'call>, A) -> NativeResult<R>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self::new(vec![Codec::Value; arity], Codec::Value, move |call| {
            invoke(call, &entry)
        })
    }
}

fn invoke<A: FromKagariArguments, R: IntoKagari>(
    call: &mut CallContext<'_>,
    entry: &impl Fn(&mut NativeContext<'_>, A) -> NativeResult<R>,
) -> NativeResult<Value> {
    let mut context = NativeContext {
        conversion: ConversionContext::in_native_call(call)?,
        invoke_script: Some(call.invoke_script),
        function: Some(call.function),
    };
    let signature = call.function.type_signature()?;
    let result_type = &signature.result;
    // A mismatch known before invocation cannot run any argument converter or
    // callback. The final value is still checked: user adapters are not trusted.
    context.conversion.check_type::<R>(result_type)?;
    let types = &signature.params;
    let mut values = Vec::new();
    values
        .try_reserve_exact(call.arguments.len())
        .map_err(|_| RuntimeError::resource_limit("native argument values"))?;
    for slot in 0..call.arguments.len() {
        values.push(call.argument(slot)?);
    }
    A::check_types(&context.conversion, types)?;
    loans::check_receiver_storage(
        &context,
        call.function.binding.receiver_access,
        types,
        &values,
    )?;
    // Keep the complete pack through result conversion as well as decoding and
    // payload access. A loan ends before the result's first allocation/safepoint.
    let roots = context
        .runtime()
        .gc()
        .root_execution_values(values.clone())
        .ok_or_else(|| RuntimeError::module_validation("native invocation argument roots"))?;
    let arguments = A::from_arguments(&mut context.conversion, types, &values)?;
    let result = entry(&mut context, arguments)?;
    let result = context.conversion.encode_value(result_type, result);
    drop(roots);
    result
    // No safepoint occurs between dropping this scope and the caller publishing
    // the returned value. LinkedResultAdapter protects it before allocating.
}
