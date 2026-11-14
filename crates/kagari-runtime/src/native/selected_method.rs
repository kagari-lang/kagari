//! Retain the checked relationship selected for a registered native requirement.
use crate::{
    error::RuntimeError,
    execution_metadata::managed::sources::MetadataSource,
    module::dispatch::CallRoute,
    native::{
        binding::NativeResult,
        context::LinkedOperation,
        conversion::{
            FromKagari, KagariType,
            arguments::{IntoKagariArguments, KagariArguments},
        },
        declarations::SelectedCall,
        function_handle::Function,
        typed::NativeContext,
    },
    session::ExecutionOptions,
};
use std::slice;

/// A checked native requirement whose calls use the current implementation.
/// Instance methods include their receiver as the first tuple element; static
/// members use their actual parameters. Clones share checked relationship evidence.
#[derive(Debug)]
pub struct SelectedMethod<A, R> {
    function: Function<A, R>,
}

impl<A, R> Clone for SelectedMethod<A, R> {
    fn clone(&self) -> Self {
        Self {
            function: self.function.clone(),
        }
    }
}

impl NativeContext<'_> {
    /// Resolve an opaque registration token, never an integer witness slot.
    /// No trait search occurs here: the installed call already carries the
    /// selected implementation, associated scopes and generic environment.
    pub fn selected_method<A: KagariArguments, R: KagariType>(
        &mut self,
        required: &SelectedCall,
    ) -> NativeResult<SelectedMethod<A, R>> {
        self.poll()?;
        let function = self.function.ok_or_else(|| {
            RuntimeError::module_validation("context has no native callable requirements")
        })?;
        let owner = self.conversion.owner();
        function.check_requirement(owner, required)?;
        let target = function
            .selected
            .get(required.slot)
            .and_then(LinkedOperation::ready)
            .ok_or_else(|| {
                RuntimeError::module_validation("native callable requirement is not ready")
            })?;
        let mut scratch = self.conversion.scratch()?;
        let environment = match &target.environment {
            Some(facts) => {
                let revision = MetadataSource::Revision {
                    owner,
                    source: scratch.owner_origin(),
                };
                let pair = scratch
                    .owner_metadata()
                    .map(|metadata| [revision, metadata]);
                let sources = pair
                    .as_ref()
                    .map_or(slice::from_ref(&revision), |pair| pair.as_slice());
                let execution = scratch.execution_id()?;
                let environment = scratch
                    .runtime_mut()
                    .copy_environment_in(execution, facts, sources)?
                    .ok_or_else(|| {
                        RuntimeError::module_validation(
                            "selected method environment is not protected",
                        )
                    })?;
                Some(scratch.runtime_mut().retain_environment(&environment)?)
            }
            None => None,
        };
        Ok(SelectedMethod {
            function: Function::from_selected(scratch.runtime_mut(), owner, target, environment)?,
        })
    }
}

impl<A, R> SelectedMethod<A, R> {
    pub fn call_route(&self) -> Option<&CallRoute> {
        self.function.call_route()
    }
}

impl<A: IntoKagariArguments, R: FromKagari> SelectedMethod<A, R> {
    pub fn call(&self, cx: &mut NativeContext<'_>, arguments: A) -> NativeResult<R> {
        self.function.call(cx, arguments)
    }

    pub fn call_with_options(
        &self,
        cx: &mut NativeContext<'_>,
        arguments: A,
        options: ExecutionOptions,
    ) -> NativeResult<R> {
        self.function.call_with_options(cx, arguments, options)
    }
}
