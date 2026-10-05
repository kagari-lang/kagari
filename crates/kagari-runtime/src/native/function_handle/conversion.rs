use crate::{
    error::RuntimeError,
    frame::types::arguments::{ScopedSignature, TypeArgument},
    module::ModuleEpochRetention,
    native::{
        binding::NativeResult,
        callable::PreparedClosure,
        catalog::DeclarationCatalog,
        conversion::{
            FromKagari, IntoKagari, KagariType, arguments::KagariArguments,
            context::ConversionContext,
        },
        function_handle::{PinnedFunction, PreparedFunction, Target},
        types::Type,
    },
    value::Value,
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

impl<A: KagariArguments, R: KagariType> KagariType for PinnedFunction<A, R> {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        Ok(Type::function(
            A::argument_types(catalog)?,
            R::kagari_type(catalog)?,
        ))
    }

    fn check_type(cx: &ConversionContext<'_>, expected: &TypeArgument) -> NativeResult<()> {
        let signature = signature(cx, expected)?;
        A::check_types(cx, &signature.params)?;
        cx.check_type::<R>(&signature.result)
    }
}

impl<A: KagariArguments, R: KagariType> FromKagari for PinnedFunction<A, R> {
    const PRESERVES_IDENTITY: bool = true;

    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &Value,
    ) -> NativeResult<Self> {
        cx.check_type::<Self>(expected)?;
        cx.check_value(expected, value)?;
        let prepared = PreparedClosure::from_value(cx.runtime(), value.clone())?;
        let owner = prepared.snapshot(cx.runtime())?.implementation.clone();
        let signature = signature(cx, expected)?;
        let root = cx
            .runtime()
            .root_value(value.clone())
            .ok_or_else(|| RuntimeError::module_validation("closure handle retention"))?;
        let program = cx
            .runtime()
            .retain_program(&owner, ModuleEpochRetention::RuntimeValue)
            .ok_or_else(|| RuntimeError::module_validation("closure program retention"))?;
        Ok(Self {
            prepared: Arc::new(PreparedFunction {
                owner,
                signature: Arc::new(signature),
                target: Target::Closure { prepared, root },
                _program: program,
            }),
            mapping: PhantomData,
        })
    }
}

impl<A: KagariArguments, R: KagariType> IntoKagari for PinnedFunction<A, R> {
    fn into_kagari(
        self,
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
    ) -> NativeResult<Value> {
        cx.check_type::<Self>(expected)?;
        self.prepared.validate(cx.runtime())?;
        let value = match &self.prepared.target {
            Target::Closure { root, .. } => root
                .value(cx.runtime().gc())
                .ok_or_else(|| RuntimeError::module_validation("closure handle root"))?,
            Target::Entry(target) => match target.target {
                CallableTarget::Script(function) => cx.runtime().make_closure(
                    self.owner(),
                    function,
                    vec![],
                    target.environment.clone(),
                )?,
                CallableTarget::Native(_) => cx.runtime().make_native_closure(
                    self.owner(),
                    target,
                    self.prepared.signature.clone(),
                )?,
            },
        };
        cx.check_value(expected, &value)?;
        Ok(value)
    }
}
