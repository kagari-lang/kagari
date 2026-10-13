//! Promote compiler-selected evidence to a retained, checked execution entry.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::arguments::ScopedSignature,
    gc::roots::RootedValue,
    module::{LoadedModule, ModuleEpochRetention},
    native::{
        binding::NativeResult,
        context::{LinkedCallable, ScriptCall},
        conversion::{KagariType, arguments::KagariArguments, context::ConversionContext},
        function_handle::{PinnedFunction, PreparedFunction, Target},
        stored_selection::StoredSelection,
        typed::NativeContext,
    },
    value::Value,
};
use std::{marker::PhantomData, slice, sync::Arc};

impl<A: KagariArguments, R: KagariType> PinnedFunction<A, R> {
    pub(crate) fn from_selected(
        runtime: &Runtime,
        caller: &LoadedModule,
        target: &LinkedCallable,
    ) -> NativeResult<Self> {
        let target = StoredSelection::new(caller, target)?.retain(runtime)?;
        let prepared = PreparedFunction::selected(runtime, caller, target)?;
        let cx = ConversionContext::new(runtime, &prepared.owner)?;
        A::check_types(&cx, &prepared.signature.params)?;
        cx.check_type::<R>(&prepared.signature.result)?;
        Ok(Self {
            prepared: Arc::new(prepared),
            mapping: PhantomData,
        })
    }
}

impl PreparedFunction {
    pub(crate) fn selected(
        runtime: &Runtime,
        caller: &LoadedModule,
        target: LinkedCallable,
    ) -> NativeResult<Self> {
        let owner = target.owner(caller)?;
        runtime.validate_loaded_module(&owner)?;
        let signature = match &target.scoped_signature {
            Some(signature) => signature.clone(),
            None => Arc::new(ScopedSignature {
                params: runtime.resolve_type_arguments(&owner, &target.params)?,
                result: runtime
                    .resolve_type_arguments(&owner, slice::from_ref(&target.result))?
                    .pop()
                    .ok_or_else(|| RuntimeError::module_validation("selected result scope"))?,
            }),
        };
        let program = runtime
            .retain_program(&owner, ModuleEpochRetention::RuntimeValue)
            .ok_or_else(|| RuntimeError::module_validation("selected program retention"))?;
        Ok(Self {
            owner,
            signature,
            target: Target::Entry(target),
            _program: program,
        })
    }

    pub(crate) fn call_values(
        &self,
        cx: &NativeContext<'_>,
        values: &[Value],
    ) -> NativeResult<RootedValue> {
        let runtime = cx.runtime();
        self.validate(runtime)?;
        let Target::Entry(target) = &self.target else {
            return Err(RuntimeError::module_validation("selected entry expected"));
        };
        if values.len() != self.signature.params.len()
            || values
                .iter()
                .enumerate()
                .any(|(index, value)| !target.matches_argument(runtime, &self.owner, index, value))
        {
            return Err(RuntimeError::module_validation("selected call arguments"));
        }
        let _arguments = runtime
            .gc()
            .root_execution_values(values.to_vec())
            .ok_or_else(|| RuntimeError::module_validation("selected argument retention"))?;
        let _session = runtime.begin_pinned_execution(self)?;
        let value = match target.primitive {
            Some(primitive) => runtime
                .invoke_standard_builtin(&self.owner, primitive, values)
                .map_err(|error| error.into_runtime_error())?,
            None => cx.invoke_script.ok_or_else(|| {
                RuntimeError::module_validation("call context has no execution backend")
            })?(runtime, &self.owner, ScriptCall::Pinned(self), values)?,
        };
        if !target.matches_result(runtime, &self.owner, &value) {
            return Err(RuntimeError::module_validation("selected call result"));
        }
        runtime
            .root_value(value)
            .ok_or_else(|| RuntimeError::module_validation("selected result retention"))
    }
}
