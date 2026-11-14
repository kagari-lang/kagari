//! Fixed-pair library policy, without interpreter entry or retained heap borrows.
mod context;
#[cfg(test)]
mod tests;
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    module::ExecutableModule,
    native::{binding::NativeResult, context::NativeCall, future::ColdFuture},
    session::ExecutionPhase,
    value::{carriers::RootedValue, view::ValueView},
    value_check,
};
use kagari_types::{
    declaration::{TypeDefKind, native::NativeStorageLayout},
    ty::Ty,
};
use std::{fmt::Debug, sync::Arc};

/// Trusted native-library policy. Implementations retain declaration handles only,
/// never script values, runtime references or mutable execution state. Both methods
/// are synchronous; the context deliberately supplies no script invoker or driver.
pub trait PairPolicy: Debug + Send + Sync {
    /// Check the closed input/output relationship before constructing a cold Future.
    fn validate(&self, cx: &PairContext<'_>) -> NativeResult<()>;

    /// Observe a newly committed normal output. Execution failures bypass policy.
    fn on_output(&self, cx: &mut PairContext<'_>, index: usize) -> NativeResult<PairDecision>;
}

/// Finish selects a rooted value; Runtime closes siblings before publishing it.
pub enum PairDecision {
    Continue,
    Finish(RootedValue),
}

#[derive(Debug, Clone)]
pub(crate) struct ColdComposition {
    pub owner: ExecutableModule,
    pub inputs: [TypeArgument; 2],
    pub output: TypeArgument,
    pub policy: Arc<dyn PairPolicy>,
}

/// Synchronous access to checked types and Runtime-rooted branch outputs. Values
/// read here remain rooted by the group. Newly allocated aggregates return roots.
pub struct PairContext<'a> {
    pub(crate) runtime: &'a mut Runtime,
    pub(crate) contract: &'a ColdComposition,
    pub(crate) outputs: &'a [Option<RootedValue>; 2],
}

impl<'call> NativeCall<'call> {
    /// Construct a cold two-input composition using the linked native signature.
    /// No branch is claimed or started until the returned Future is driven.
    pub fn compose_pair(&mut self, policy: Arc<dyn PairPolicy>) -> NativeResult<ValueView<'call>> {
        self.runtime
            .reject_static_initialization("cold composition construction")?;
        self.runtime.resources().poll_execution()?;
        if self.runtime.execution_options().phase != ExecutionPhase::Ordinary {
            return Err(RuntimeError::execution_phase_violation(
                "cold composition construction",
            ));
        }
        let signature = self.function.type_signature()?;
        if signature.params.len() != 2 {
            return Err(invalid());
        }
        let output = future_output(self.runtime, self.owner, &signature.result)?;
        let inputs = [
            future_output(self.runtime, self.owner, &signature.params[0])?,
            future_output(self.runtime, self.owner, &signature.params[1])?,
        ];
        let values = [self.argument(0)?, self.argument(1)?];
        for (ty, value) in signature.params.iter().zip(values.iter()) {
            if !value_check::matches_protected(self.runtime, self.owner, ty, value)? {
                return Err(invalid());
            }
        }
        let cold = ColdComposition {
            owner: self.owner.clone(),
            inputs,
            output,
            policy,
        };
        cold.policy.validate(&PairContext {
            runtime: self.runtime,
            contract: &cold,
            outputs: &[None, None],
        })?;
        self.allocate_future_result(ColdFuture::Composition(cold), &values)
    }
}

pub(crate) fn future_output(
    runtime: &Runtime,
    owner: &ExecutableModule,
    ty: &TypeArgument,
) -> NativeResult<TypeArgument> {
    let Ty::NativeObject(nominal) = ty.ty() else {
        return Err(invalid());
    };
    if nominal.arguments.len() != 1
        || !nominal.associated_types.is_empty()
        || runtime
            .native_entries
            .catalog
            .types
            .get_id(nominal.declaration)
            .is_none_or(|record| {
                record.kind != TypeDefKind::NativeStorage(NativeStorageLayout::Future)
            })
    {
        return Err(invalid());
    }
    ty.parameter(runtime, owner, 0)
}

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("fixed-pair composition contract")
}
