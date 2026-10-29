//! Retain the checked relationship selected for a registered native requirement.
use crate::{
    error::RuntimeError,
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
        &self,
        required: &SelectedCall,
    ) -> NativeResult<SelectedMethod<A, R>> {
        self.runtime().gc().ensure_no_storage_borrow()?;
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
        Ok(SelectedMethod {
            function: Function::from_selected(self.runtime(), owner, target)?,
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
