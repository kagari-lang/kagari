//! Formal shared access publishes an inert alias, preserving the caller's capability.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};

use crate::{
    error::RuntimeError,
    frame::types::arguments::ScopedSignature,
    native::{
        binding::{LinkedNativeFunction, NativeResult, SharedParameter},
        context::CallContext,
    },
    value::Value,
};
use kagari_types::{declaration::host::HostPassingStyle, reference::ReferenceAccess, ty::Ty};

impl LinkedNativeFunction {
    pub(crate) fn prepare_shared_parameters(
        &self,
        signature: &ScopedSignature,
    ) -> Box<[(usize, SharedParameter)]> {
        signature
            .params
            .iter()
            .enumerate()
            .filter_map(|(index, parameter)| {
                let external_shared = self.binding.host_policy.as_ref().is_some_and(|policy| {
                    policy.passing.get(index) == Some(&HostPassingStyle::SharedBorrow)
                        && matches!(parameter.ty(), Ty::Host(_))
                });
                if external_shared {
                    Some((index, SharedParameter::Host))
                } else if matches!(
                    parameter.ty(),
                    Ty::Reference {
                        access: ReferenceAccess::Shared,
                        ..
                    }
                ) || (index == 0
                    && self.binding.receiver_access == Some(ReferenceAccess::Shared))
                {
                    Some((index, SharedParameter::Scoped))
                } else {
                    None
                }
            })
            .collect()
    }
}

impl CallContext<'_> {
    pub(crate) fn narrow_arguments(&mut self) -> NativeResult<()> {
        let prepared =
            self.function.prepared_signature.get().ok_or_else(|| {
                RuntimeError::module_validation("native signature is not prepared")
            })?;
        let signature = self.function.type_signature()?;
        for &(index, role) in &prepared.shared_parameters {
            #[cfg(feature = "execution-diagnostics")]
            diagnostics::record(Event::NativeParameterScan);
            let parameter = &signature.params[index];
            let original = self.argument(index)?;
            if !matches!(original, Value::Ephemeral(_) | Value::HostPathView(_)) {
                continue;
            }
            // Validate the live capability before boolean type matching can
            // erase an expired/foreign borrow's diagnostic. The alias remains
            // inert until the declaration check succeeds and it is rooted below.
            let narrowed = if matches!(role, SharedParameter::Host) {
                self.runtime.narrow_host_argument(&original)?
            } else {
                self.runtime
                    .narrow_scoped(&original, ReferenceAccess::Shared)?
            };
            if !parameter.matches(self.runtime, &original, self.owner)
                && !self.runtime.matches_scoped_receiver(
                    &original,
                    parameter.ty(),
                    self.owner,
                    None,
                    ReferenceAccess::Shared,
                )
            {
                return Err(invalid());
            }
            if let Some(narrowed) = narrowed {
                // Only this callee's frozen parameter is narrowed. The caller's
                // original capability and other aliases remain unchanged.
                self.arguments
                    .frame
                    .set(self.heap(), index + 1, narrowed)
                    .ok_or_else(invalid)?;
            }
        }
        Ok(())
    }
}

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("native shared argument capability")
}
