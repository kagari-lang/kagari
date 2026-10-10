//! Concrete Rust callbacks share the checked conversion boundary with host access.
mod receiver;
use crate::{
    Runtime,
    error::RuntimeError,
    gc::GcCollection,
    module::LoadedModule,
    native::{
        binding::{Codec, LinkedNativeFunction, NativeBinding, NativeResult},
        catalog::DeclarationCatalog,
        context::{CallContext, ScriptInvoker},
        conversion::{
            FromKagari, IntoKagari, arguments::FromKagariArguments, context::ConversionContext,
        },
        typed::receiver::ReceiverArguments,
    },
    value::Value,
};
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
    pub fn new(runtime: &'call Runtime, owner: &'call LoadedModule) -> NativeResult<Self> {
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
        owner: &'call LoadedModule,
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
    /// Ordinary authored registrations should use ModuleBuilder::bind_typed.
    pub fn declared<A: FromKagariArguments, R: IntoKagari>(
        declaration: &NativeDeclaration,
        entry: impl for<'call> Fn(&mut NativeContext<'call>, A) -> NativeResult<R>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        let mut binding = Self::contextual(declaration.function.params.len(), entry);
        binding.requirement_owner = Some(declaration.declaration.clone());
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

    /// Instance callbacks receive their receiver separately from the outer
    /// argument tuple. The checked declaration still owns the receiver's type.
    pub fn typed_method<S, A, R>(
        catalog: &DeclarationCatalog,
        entry: impl for<'call> Fn(&mut NativeContext<'call>, S, A) -> NativeResult<R>
        + Send
        + Sync
        + 'static,
    ) -> NativeResult<Self>
    where
        S: FromKagari,
        A: FromKagariArguments,
        R: IntoKagari,
    {
        Self::typed(
            catalog,
            move |cx, ReceiverArguments(receiver, arguments)| entry(cx, receiver, arguments),
        )
    }

    /// Bind an outer argument tuple and one fallibly converted result. The
    /// declaration builder checks this binding against the exported signature.
    pub fn typed<A, R>(
        catalog: &DeclarationCatalog,
        entry: impl for<'call> Fn(&mut NativeContext<'call>, A) -> NativeResult<R>
        + Send
        + Sync
        + 'static,
    ) -> NativeResult<Self>
    where
        A: FromKagariArguments,
        R: IntoKagari,
    {
        let arguments = A::argument_types(catalog)?
            .into_iter()
            .map(|ty| Codec::Scalar(ty.0))
            .collect::<Vec<_>>();
        let result = Codec::Scalar(R::kagari_type(catalog)?.0);
        Ok(Self::new(arguments, result, move |call| {
            invoke(call, &entry)
        }))
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
    let arguments = A::from_arguments(&mut context.conversion, types, &values)?;
    let result = entry(&mut context, arguments)?;
    context.conversion.encode_value(result_type, result)
    // No safepoint occurs between dropping this scope and the caller publishing
    // the returned value. LinkedResultAdapter protects it before allocating.
}
