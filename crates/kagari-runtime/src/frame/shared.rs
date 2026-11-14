//! Admit an exact callback or host selection without selecting its route again.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::{FrameDispatch, FrameEntry, FrameEnvironment, FrameStack, arguments::FrameArguments},
    native::{
        context::SelectedCall,
        conversion::{arguments::ArgumentValues, context::ConversionContext},
    },
};

use std::borrow::Cow;

impl FrameStack {
    /// Admit an exact callback/host selection. Its producer owns current-route
    /// selection and environment transport; retained queued work must not reselect.
    pub fn push_selected_call(
        &self,
        runtime: &mut Runtime,
        selected: &SelectedCall<'_>,
        args: ArgumentValues<'_>,
    ) -> Result<(), RuntimeError> {
        let owner = selected.owner().clone();
        let target = selected.target();
        runtime.validate_executable_module(&owner)?;
        if args.len() != target.params.len() {
            return Err(RuntimeError::module_validation("selected call arguments"));
        }
        // Prepared selections retain their lexical signature. Only an unchecked
        // closed selection needs preparation at this dynamic call boundary.
        let parameters = match target.scoped_signature.get() {
            Some(signature) => Cow::Borrowed(signature.params.as_slice()),
            None => Cow::Owned(runtime.resolve_type_arguments(&owner, &target.params)?),
        };
        if parameters.len() != args.len() {
            return Err(RuntimeError::module_validation(
                "selected call signature arity",
            ));
        }
        let mut conversion = ConversionContext::for_selected(
            runtime,
            &owner,
            self.session,
            selected.origin(),
            selected.environment_source(),
        )?;
        let environment = match (&target.environment, selected.environment_source()) {
            (Some(facts), Some(source)) => Some(FrameEnvironment {
                facts: facts.clone(),
                source,
            }),
            (None, _) => None,
            _ => {
                return Err(RuntimeError::module_validation(
                    "selected frame environment source",
                ));
            }
        };
        for (index, expected) in parameters.iter().enumerate() {
            if index != 0 && index.is_multiple_of(1024) {
                conversion.poll()?;
            }
            let value = args.get(&mut conversion, index)?;
            conversion.check_value(expected, &value)?;
        }
        // A conversion mark cannot retire across an admitted live frame. Only
        // original sources/facts cross admission; no preparation coordinate is
        // created here, and the frame publishes its own environment dependency.
        drop(conversion);
        self.push_arguments(
            runtime,
            owner.clone(),
            target.target,
            FrameArguments::pack(args),
            None,
            FrameDispatch {
                origin: selected.origin(),
                metadata: selected.environment_source(),
                prepared: None,
                entry: FrameEntry::Call,
                invocation: None,
                environment,
            },
        )
    }
}
