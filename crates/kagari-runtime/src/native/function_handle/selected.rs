//! Promote compiler-selected evidence to a retained, checked execution entry.
use crate::{
    Runtime,
    error::RuntimeError,
    execution_metadata::MetadataRoot,
    frame::types::arguments::ScopedSignature,
    gc::roots::{RootSet, RootedValue},
    module::{ExecutableModule, ModuleEpochRetention, dispatch::CallRoute},
    native::{
        binding::NativeResult,
        context::{CallableOwner, LinkedCallable, LinkedOperation, ScriptCall},
        conversion::{KagariType, arguments::KagariArguments, context::ConversionContext},
        function_handle::{Function, FunctionBinding, PreparedFunction, Target},
        typed::NativeContext,
    },
    value::Value,
};
use std::{marker::PhantomData, slice, sync::Arc};

/// The already checked relationship and lexical environment, not a selected body lease.
#[derive(Debug)]
pub(super) struct SelectedBinding {
    pub(super) route: CallRoute,
    owner: ExecutableModule,
    target: LinkedCallable,
    _roots: RootSet,
}

impl SelectedBinding {
    pub(super) fn prepare(&self, runtime: &Runtime) -> NativeResult<PreparedFunction> {
        let selected = runtime.select_call_route(&self.route)?;
        let mut target = self.target.clone();
        target.environment = target
            .environment
            .as_ref()
            .map(|environment| runtime.transport_environment(&self.owner, &selected, environment))
            .transpose()?;
        target.target = selected.target();
        let mut roots = vec![MetadataRoot::Program(selected.owner().clone())];
        roots.extend(
            target
                .environment
                .iter()
                .map(|environment| MetadataRoot::Environment(environment.id)),
        );
        target.owner =
            CallableOwner::Pinned(selected.owner().clone(), runtime.root_metadata(roots)?);
        PreparedFunction::selected(runtime, selected.owner(), target)
    }
}

impl NativeContext<'_> {
    /// Retain a typed relationship supplied by this checked native invocation.
    /// Ordinary calls select current bodies; PreparedFunction pins one computation.
    pub fn selected_function<A: KagariArguments, R: KagariType>(
        &self,
        slot: usize,
    ) -> NativeResult<Function<A, R>> {
        self.runtime().gc().ensure_execution_allowed()?;
        let selected = self
            .function
            .and_then(|function| function.selected.get(slot))
            .and_then(LinkedOperation::ready)
            .ok_or_else(|| RuntimeError::module_validation("native selected callable slot"))?;
        Function::from_selected(self.runtime(), self.conversion.owner(), selected)
    }
}

impl<A: KagariArguments, R: KagariType> Function<A, R> {
    pub(crate) fn from_selected(
        runtime: &Runtime,
        caller: &ExecutableModule,
        target: &LinkedCallable,
    ) -> NativeResult<Self> {
        let prepared = PreparedFunction::selected(runtime, caller, target.clone())?;
        let cx = ConversionContext::new(runtime, &prepared.owner)?;
        A::check_types(&cx, &prepared.signature.params)?;
        cx.check_type::<R>(&prepared.signature.result)?;
        let owner = prepared.owner.clone();
        let route = owner.bind_call_route(target.target)?;
        let mut target = target.clone();
        target.owner = CallableOwner::Program(owner.slot());
        target.scoped_signature = Some(prepared.signature.clone());
        let roots = runtime.root_metadata(
            target
                .environment
                .iter()
                .map(|environment| MetadataRoot::Environment(environment.id))
                .collect(),
        )?;
        Ok(Self {
            binding: Arc::new(FunctionBinding::Selected(SelectedBinding {
                route,
                owner,
                target,
                _roots: roots,
            })),
            mapping: PhantomData,
        })
    }
}

impl PreparedFunction {
    pub(crate) fn selected(
        runtime: &Runtime,
        caller: &ExecutableModule,
        target: LinkedCallable,
    ) -> NativeResult<Self> {
        let owner = target.owner(caller)?;
        runtime.validate_executable_module(&owner)?;
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
            source_entry: target.target,
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
