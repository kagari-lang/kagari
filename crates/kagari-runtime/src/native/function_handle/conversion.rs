use crate::{
    Runtime,
    closure::ClosureMode,
    error::RuntimeError,
    execution_metadata::managed::sources::MetadataSource,
    frame::types::arguments::{ScopedSignature, TypeArgument},
    module::{ModuleEpochRetention, managed::source::RevisionSource},
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        conversion::{
            FromKagari, IntoKagari, KagariType, arguments::KagariArguments,
            context::ConversionContext,
        },
        function_handle::{Function, FunctionBinding, PreparedFunction, Target},
        types::Type,
    },
    value::{carriers::RootedValue, view::ValueView},
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
        value: &ValueView<'_>,
    ) -> NativeResult<Self> {
        cx.check_type::<Self>(expected)?;
        cx.check_value(expected, value)?;
        let record = cx.runtime_mut().resolve_closure(value)?;
        if let Some(route) = record.call_route() {
            cx.runtime().select_call_route(route)?;
            let root = cx.retain(value)?;
            return Ok(Self {
                binding: Arc::new(FunctionBinding::NamedClosure {
                    route: route.clone(),
                    root,
                }),
                mapping: PhantomData,
            });
        }
        let owner = record.implementation.clone();
        let location = value
            .location()
            .ok_or_else(|| RuntimeError::module_validation("closure source"))?;
        let program = cx
            .runtime_mut()
            .retain_program(
                &owner,
                ModuleEpochRetention::RuntimeValue,
                RevisionSource::Closure(location),
            )
            .ok_or_else(|| RuntimeError::module_validation("closure program retention"))?;
        let root = cx.retain(value)?;
        Ok(Self {
            binding: Arc::new(FunctionBinding::Closure(Arc::new(PreparedFunction {
                source_entry: record.callable_target(),
                owner,
                signature: record.signature().clone(),
                target: Target::Closure { root },
                _program: program,
                environment: None,
            }))),
            mapping: PhantomData,
        })
    }
}

impl<A, R> Function<A, R> {
    pub(super) fn prepare_named_closure(
        &self,
        runtime: &mut Runtime,
        root: &RootedValue,
    ) -> NativeResult<PreparedFunction> {
        // A Host scope imports the actual object before choosing a current body.
        // It needs no old-body descriptor to create its temporary source owner.
        let mut scope = runtime.host_scope(&[])?;
        let value = scope.import(root)?;
        let execution = value
            .location()
            .ok_or_else(|| RuntimeError::module_validation("closure root"))?
            .execution;
        let selected = scope.runtime_mut().select_closure_call(execution, &value)?;
        let mut conversion = ConversionContext::for_selected(
            scope.runtime_mut(),
            selected.owner(),
            execution,
            selected.origin(),
            selected.environment().map(|(_, source)| source),
        )?;
        let frozen = conversion.freeze_closure(&selected)?;
        let root = conversion.retain(&frozen)?;
        let program = conversion
            .runtime_mut()
            .retain_program(
                selected.owner(),
                ModuleEpochRetention::RuntimeValue,
                selected.origin(),
            )
            .ok_or_else(|| RuntimeError::module_validation("selected closure program retention"))?;
        Ok(PreparedFunction {
            source_entry: selected.target(),
            owner: selected.owner().clone(),
            signature: selected.record().signature().clone(),
            target: Target::Closure { root },
            _program: program,
            environment: None,
        })
    }
}

impl<A: KagariArguments, R: KagariType> IntoKagari for Function<A, R> {
    fn into_kagari<'call>(
        self,
        cx: &mut ConversionContext<'call>,
        expected: &TypeArgument,
    ) -> NativeResult<ValueView<'call>> {
        cx.check_type::<Self>(expected)?;
        if let FunctionBinding::NamedClosure { root, .. } = self.binding.as_ref() {
            let value = cx.import(root)?;
            cx.check_value(expected, &value)?;
            return Ok(value);
        }
        let prepared = self.prepare_call(cx.runtime_mut())?;
        let prepared = &prepared.selection;
        if let Target::Closure { root } = &prepared.target {
            let value = cx.import(root)?;
            cx.check_value(expected, &value)?;
            return Ok(value);
        }
        let Target::Entry(target) = &prepared.target else {
            unreachable!()
        };
        let execution = cx.execution_id()?;
        let value = {
            let mut conversion = ConversionContext::for_selected(
                cx.runtime_mut(),
                prepared.owner(),
                execution,
                prepared.body_origin(),
                prepared
                    .environment
                    .as_ref()
                    .map(MetadataSource::HostEnvironment),
            )?;
            let value = match target.target {
                CallableTarget::Script(function) => conversion.allocate_script_closure(
                    function,
                    &[],
                    target.environment.clone(),
                    ClosureMode::Ordinary,
                )?,
                CallableTarget::Native(_) => conversion.allocate_native_closure(
                    target,
                    prepared.signature.clone(),
                    ClosureMode::Ordinary,
                )?,
            };
            conversion.into_native_value(execution, value)?
        };
        let value = value.reborrow();
        cx.check_value(expected, &value)?;
        Ok(value)
    }
}
