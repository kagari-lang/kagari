//! Formal shared access publishes an inert alias, preserving the caller's capability.
use crate::{
    error::RuntimeError,
    native::{binding::NativeResult, context::CallContext},
    value::Value,
};
use kagari_types::{declaration::host::HostPassingStyle, reference::ReferenceAccess, ty::Ty};

impl CallContext<'_> {
    pub(crate) fn narrow_arguments(&self) -> NativeResult<Self> {
        let mut arguments = self.arguments.clone();
        let signature = self.function.type_signature()?;
        for (index, parameter) in signature.params.iter().enumerate() {
            let external_shared =
                self.function
                    .binding
                    .host_policy
                    .as_ref()
                    .is_some_and(|policy| {
                        policy.passing.get(index) == Some(&HostPassingStyle::SharedBorrow)
                            && matches!(parameter.ty(), Ty::Host(_))
                    });
            let shared = matches!(
                parameter.ty(),
                Ty::Reference {
                    access: ReferenceAccess::Shared,
                    ..
                }
            ) || (index == 0
                && self.function.binding.receiver_access == Some(ReferenceAccess::Shared))
                || external_shared;
            if !shared {
                continue;
            }
            let original = self.argument(index)?;
            if !matches!(original, Value::Ephemeral(_) | Value::HostPathView(_)) {
                continue;
            }
            // Validate the live capability before boolean type matching can
            // erase an expired/foreign borrow's diagnostic. The alias remains
            // inert until the declaration check succeeds and it is rooted below.
            let narrowed = if external_shared {
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
                if arguments.narrowed.is_none() {
                    let values = (0..arguments.len())
                        .map(|slot| arguments.get(slot).ok_or_else(invalid))
                        .collect::<NativeResult<Vec<_>>>()?;
                    arguments.narrowed = Some(
                        self.heap()
                            .root_execution_values(values)
                            .ok_or_else(invalid)?,
                    );
                }
                arguments
                    .narrowed
                    .as_ref()
                    .ok_or_else(invalid)?
                    .set(self.heap(), index, narrowed)
                    .ok_or_else(invalid)?;
            }
        }
        Ok(Self {
            runtime: self.runtime,
            owner: self.owner,
            function: self.function,
            arguments,
            invoke_script: self.invoke_script,
        })
    }
}

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("native shared argument capability")
}
