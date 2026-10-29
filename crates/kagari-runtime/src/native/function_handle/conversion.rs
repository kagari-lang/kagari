use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::arguments::{ScopedSignature, TypeArgument},
    gc::roots::RootedValue,
    module::ModuleEpochRetention,
    native::{
        binding::NativeResult,
        callable::PreparedClosure,
        catalog::DeclarationCatalog,
        conversion::{
            FromKagari, IntoKagari, KagariType, arguments::KagariArguments,
            context::ConversionContext,
        },
        function_handle::{Function, FunctionBinding, PreparedFunction, Target},
        types::Type,
        value_handle::ScriptValue,
    },
};
use kagari_bytecode::module::CallableTarget;
use kagari_types::ty::Ty;
use std::{marker::PhantomData, sync::Arc};

fn signature(cx: &ConversionContext<'_>, expected: &TypeArgument) -> NativeResult<ScopedSignature> {
    let Ty::Function { params, .. } = expected.ty() else {
        return Err(RuntimeError::module_validation(
            "function handle requires a function type",
        ));
    };
    let params = (0..params.len())
        .map(|index| {
            expected.derive(cx.runtime(), cx.owner(), |ty| match ty {
                Ty::Function { params, .. } => params.get(index).cloned(),
                _ => None,
            })
        })
        .collect::<NativeResult<Vec<_>>>()?;
    let result = expected.derive(cx.runtime(), cx.owner(), |ty| match ty {
        Ty::Function { result, .. } => Some(*result.clone()),
        _ => None,
    })?;
    Ok(ScopedSignature { params, result })
}

impl<A: KagariArguments, R: KagariType> KagariType for Function<A, R> {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        Type::function(A::argument_types(catalog)?, R::kagari_type(catalog)?)
    }

    fn check_type(cx: &ConversionContext<'_>, expected: &TypeArgument) -> NativeResult<()> {
        let signature = signature(cx, expected)?;
        A::check_types(cx, &signature.params)?;
        cx.check_type::<R>(&signature.result)
    }
}

impl<A: KagariArguments, R: KagariType> FromKagari for Function<A, R> {
    const PRESERVES_IDENTITY: bool = true;

    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &ScriptValue,
    ) -> NativeResult<Self> {
        cx.check_type::<Self>(expected)?;
        let raw = value.call_argument(cx.runtime(), expected, cx.owner())?;
        let value = &raw;
        cx.check_value(expected, value)?;
        let prepared = PreparedClosure::from_value(cx.runtime(), *value)?;
        let snapshot = prepared.snapshot(cx.runtime())?;
        let owner = snapshot.implementation.clone();
        let route = snapshot.call_route().cloned();
        let source_entry = snapshot.callable_target();
        drop(snapshot);
        if let Some(route) = &route {
            cx.runtime().select_call_route(route)?;
        }
        // A named value root retains captures/evidence, not its old executable.
        let root = cx
            .runtime()
            .root_value(*value)
            .ok_or_else(|| RuntimeError::module_validation("closure handle retention"))?;
        if let Some(route) = route {
            return Ok(Self {
                binding: Arc::new(FunctionBinding::NamedClosure { route, root }),
                mapping: PhantomData,
            });
        }
        let signature = signature(cx, expected)?;
        let program = cx
            .runtime()
            .retain_program(&owner, ModuleEpochRetention::RuntimeValue)
            .ok_or_else(|| RuntimeError::module_validation("closure program retention"))?;
        Ok(Self {
            binding: Arc::new(FunctionBinding::Closure(Arc::new(PreparedFunction {
                source_entry,
                owner,
                signature: Arc::new(signature),
                target: Target::Closure { prepared, root },
                _program: program,
            }))),
            mapping: PhantomData,
        })
    }
}

impl<A, R> Function<A, R> {
    pub(super) fn prepare_named_closure(
        &self,
        runtime: &Runtime,
        root: &RootedValue,
    ) -> NativeResult<PreparedFunction> {
        let value = root
            .value(runtime.gc())
            .ok_or_else(|| RuntimeError::module_validation("closure handle root"))?;
        let (prepared, root) =
            PreparedClosure::from_value(runtime, value)?.prepare_call(runtime)?;
        let snapshot = prepared.snapshot(runtime)?;
        let signature = snapshot.signature(runtime)?;
        let source_entry = snapshot.callable_target();
        let owner = snapshot.implementation.clone();
        drop(snapshot);
        let program = runtime
            .retain_program(&owner, ModuleEpochRetention::RuntimeValue)
            .ok_or_else(|| RuntimeError::module_validation("selected closure program retention"))?;
        Ok(PreparedFunction {
            source_entry,
            owner,
            signature,
            target: Target::Closure { prepared, root },
            _program: program,
        })
    }
}

impl<A: KagariArguments, R: KagariType> IntoKagari for Function<A, R> {
    fn into_kagari(
        self,
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
    ) -> NativeResult<ScriptValue> {
        cx.check_type::<Self>(expected)?;
        if let FunctionBinding::NamedClosure { root, .. } = self.binding.as_ref() {
            let value = root
                .value(cx.runtime().gc())
                .ok_or_else(|| RuntimeError::module_validation("closure handle root"))?;
            cx.check_value(expected, &value)?;
            return Ok(ScriptValue::from_root(
                root.clone(),
                expected.clone(),
                cx.owner().clone(),
            ));
        }
        let prepared = self.prepare_call(cx.runtime())?;
        let value = match &prepared.target {
            Target::Closure { root, .. } => root
                .value(cx.runtime().gc())
                .ok_or_else(|| RuntimeError::module_validation("closure handle root"))?,
            Target::Entry(target) => match target.target {
                CallableTarget::Script(function) => cx.runtime().make_closure(
                    prepared.owner(),
                    function,
                    vec![],
                    target.environment.clone(),
                )?,
                CallableTarget::Native(_) => cx.runtime().make_native_closure(
                    prepared.owner(),
                    target,
                    prepared.signature.clone(),
                )?,
            },
        };
        cx.retain_converted(expected, value)
    }
}
