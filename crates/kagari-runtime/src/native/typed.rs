//! Concrete Rust callbacks share the checked conversion boundary with host access.
mod loan_arguments;
pub mod loans;
mod receiver;
use crate::{
    Runtime,
    error::RuntimeError,
    module::ExecutableModule,
    native::{
        binding::{Codec, LinkedNativeFunction, NativeBinding, NativeResult},
        context::{NativeCall, ScriptInvoker},
        conversion::{
            FromKagari, IntoKagari,
            arguments::{FromKagariArguments, PreparedArguments},
            context::ConversionContext,
        },
        typed::receiver::ReceiverArguments,
    },
    value::kind::ObjectKind,
};
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_gc::heap::CollectionReport;
use kagari_types::declaration::NativeDeclaration;
use std::sync::Arc;

/// One synchronous native invocation. Arguments are owned Rust data or retained
/// handles; no argument-slot or payload borrow spans the callback.
pub struct NativeContext<'call> {
    pub(crate) conversion: ConversionContext<'call>,
    pub(crate) invoke_script: Option<ScriptInvoker>,
    pub(crate) function: Option<&'call LinkedNativeFunction>,
}

impl<'call> NativeContext<'call> {
    /// Open a synchronous host access scope for a retained program version.
    pub fn new(runtime: &'call mut Runtime, owner: &'call ExecutableModule) -> NativeResult<Self> {
        Ok(Self {
            conversion: ConversionContext::new(runtime, owner)?,
            invoke_script: None,
            function: None,
        })
    }

    /// Backend integration: install synchronous execution for typed calls.
    /// Native callbacks inherit this service from their invoking backend.
    pub fn with_invoker(
        runtime: &'call mut Runtime,
        owner: &'call ExecutableModule,
        invoke_script: ScriptInvoker,
    ) -> NativeResult<Self> {
        Ok(Self {
            conversion: ConversionContext::new(runtime, owner)?,
            invoke_script: Some(invoke_script),
            function: None,
        })
    }

    pub fn runtime(&self) -> &Runtime {
        self.conversion.runtime()
    }

    pub fn runtime_mut(&mut self) -> &mut Runtime {
        self.conversion.runtime_mut()
    }

    pub fn poll(&self) -> NativeResult<()> {
        self.conversion.poll()
    }

    pub fn collect_garbage(&mut self) -> NativeResult<CollectionReport> {
        self.runtime_mut().collect_garbage()
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
    /// and binding; each linked binding or closed application checks Rust mappings
    /// before value conversion or callback effects.
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
    /// declaration builders supply the arity; linked preparation checks Rust types
    /// before argument conversion or callback effects.
    pub(crate) fn contextual<A: FromKagariArguments, R: IntoKagari>(
        arity: usize,
        entry: impl for<'call> Fn(&mut NativeContext<'call>, A) -> NativeResult<R>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        let mut binding = Self::scoped(vec![Codec::Value; arity], Codec::Value, move |call| {
            invoke(call, &entry)
        });
        binding.prepare_mapping = Some(Arc::new(|cx, signature| {
            cx.check_type::<R>(&signature.result)?;
            A::check_types(cx, &signature.params)
        }));
        binding
    }
}

fn invoke<A: FromKagariArguments, R: IntoKagari>(
    call: &mut NativeCall<'_>,
    entry: &impl Fn(&mut NativeContext<'_>, A) -> NativeResult<R>,
) -> NativeResult<()> {
    let invoke_script = call.invoke_script;
    let function = call.function;
    let signature = function.type_signature()?;
    let result_type = &signature.result;
    let types = &signature.params;
    let values = call.arguments();
    let destination = call.pending_place()?;
    let published = {
        let mut context = NativeContext {
            conversion: ConversionContext::in_native_call(call)?,
            invoke_script: Some(invoke_script),
            function: Some(function),
        };
        context.conversion.check_elements(values.len())?;
        loans::check_receiver_storage(
            &mut context,
            function.binding.receiver_access,
            types,
            values.reborrow(),
        )?;
        // Prepared mappings belong to the binding. The receiver owner admits
        // scoped external receivers separately; the ordinary matcher must not
        // broaden an owned Native/struct type to make that case pass globally.
        for (index, ty) in types.iter().enumerate() {
            if index != 0 && index.is_multiple_of(1024) {
                context.poll()?;
            }
            let value = values.get(&mut context.conversion, index)?;
            if index == 0
                && function.binding.receiver_access.is_some()
                && value.kind() == Some(ObjectKind::Ephemeral)
            {
                continue;
            }
            context.conversion.check_value(ty, &value)?;
        }
        let arguments = PreparedArguments::<A>::from_binding(&context.conversion, types)
            .decode_admitted(&mut context.conversion, values.reborrow())?;
        let result = entry(&mut context, arguments)?;
        let value = context.conversion.encode_prepared(result_type, result)?;
        // Publish while this conversion's local suffix is still alive. Its Drop
        // may retire every encoder temporary before Native completion proceeds.
        destination.publish(context.runtime_mut(), &value)?
    };
    call.accept_staged(published)
}
