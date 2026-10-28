//! Fixed-pair library policy, without interpreter entry or retained heap borrows.
#[cfg(test)]
mod tests;
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::{arguments::TypeArgument, compatibility::TypeView},
    gc::roots::RootedValue,
    module::ExecutableModule,
    native::{
        binding::NativeResult,
        context::CallContext,
        future::{ColdFuture, FuturePayload},
        types::VariantRef,
    },
    session::ExecutionPhase,
    value::{EnumTag, Value},
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
    fn on_output(&self, cx: &PairContext<'_>, index: usize) -> NativeResult<PairDecision>;
}

/// Finish selects a rooted value; Runtime closes siblings before publishing it.
pub enum PairDecision {
    Continue,
    Finish(RootedValue),
}

#[derive(Debug)]
pub(crate) struct ColdComposition {
    pub owner: ExecutableModule,
    pub inputs: [TypeArgument; 2],
    pub output: TypeArgument,
    pub values: [Value; 2],
    pub policy: Arc<dyn PairPolicy>,
}

/// Synchronous access to checked types and Runtime-rooted branch outputs. Values
/// read here remain rooted by the group. Newly allocated aggregates return roots.
pub struct PairContext<'a> {
    pub(crate) runtime: &'a Runtime,
    pub(crate) contract: &'a ColdComposition,
    pub(crate) outputs: &'a [Option<RootedValue>; 2],
}

impl PairContext<'_> {
    pub fn input_type(&self, index: usize) -> NativeResult<&TypeArgument> {
        self.contract.inputs.get(index).ok_or_else(invalid)
    }

    pub fn output_type(&self) -> &TypeArgument {
        &self.contract.output
    }

    pub fn parameter(&self, ty: &TypeArgument, index: usize) -> NativeResult<TypeArgument> {
        ty.parameter(self.runtime, &self.contract.owner, index)
    }

    pub fn compatible(&self, left: &TypeArgument, right: &TypeArgument) -> bool {
        left.view(&self.contract.owner)
            .compatible(right.view(&self.contract.owner))
    }

    pub fn check_variant(&self, ty: &TypeArgument, variant: &VariantRef) -> NativeResult<()> {
        self.runtime
            .checked_enum_variant(&self.contract.owner, ty, variant)
            .map(|_| ())
    }

    pub fn output(&self, index: usize) -> NativeResult<Option<Value>> {
        self.outputs
            .get(index)
            .ok_or_else(invalid)?
            .as_ref()
            .map(|root| self.value(root))
            .transpose()
    }

    pub fn value(&self, root: &RootedValue) -> NativeResult<Value> {
        root.value(self.runtime.gc()).ok_or_else(invalid)
    }

    pub fn is_variant(&self, index: usize, variant: &VariantRef) -> NativeResult<bool> {
        let expected = self.runtime.checked_enum_variant(
            &self.contract.owner,
            self.input_type(index)?,
            variant,
        )?;
        let Some(Value::Enum(id)) = self.output(index)? else {
            return Err(invalid());
        };
        let view = self.runtime.gc().enum_view(id).ok_or_else(invalid)?;
        Ok(matches!(&view.tag, EnumTag::Declared(actual) if actual.matches_layout(&expected)))
    }

    pub fn field(&self, index: usize, variant: &VariantRef, field: usize) -> NativeResult<Value> {
        if !self.is_variant(index, variant)? {
            return Err(invalid());
        }
        let Some(Value::Enum(id)) = self.output(index)? else {
            return Err(invalid());
        };
        let view = self.runtime.gc().enum_view(id).ok_or_else(invalid)?;
        view.fields.get(field).copied().ok_or_else(invalid)
    }

    fn root_checked(&self, ty: &TypeArgument, value: Value) -> NativeResult<RootedValue> {
        if !ty.matches(self.runtime, &value, &self.contract.owner) {
            return Err(invalid());
        }
        self.runtime.root_value(value).ok_or_else(invalid)
    }

    pub fn tuple(&self, ty: &TypeArgument, values: Vec<Value>) -> NativeResult<RootedValue> {
        let Ty::Tuple(elements) = ty.ty() else {
            return Err(invalid());
        };
        if elements.len() != values.len() {
            return Err(invalid());
        }
        for (index, value) in values.iter().enumerate() {
            if !self
                .parameter(ty, index)?
                .matches(self.runtime, value, &self.contract.owner)
            {
                return Err(invalid());
            }
        }
        self.root_checked(ty, self.runtime.gc().alloc_tuple(values)?)
    }

    pub fn allocate_enum(
        &self,
        ty: &TypeArgument,
        variant: &VariantRef,
        fields: Vec<Value>,
    ) -> NativeResult<RootedValue> {
        let layout = self
            .runtime
            .checked_enum_variant(&self.contract.owner, ty, variant)?;
        let value = self
            .runtime
            .alloc_enum(EnumTag::Declared(layout), fields)
            .map(Value::Enum)?;
        self.root_checked(ty, value)
    }

    /// Rebind the same declared variant across applied types. Payload types must
    /// match exactly; the original optional diagnostic origin is copied, not captured.
    pub fn forward_variant(
        &self,
        index: usize,
        target: &TypeArgument,
        variant: &VariantRef,
    ) -> NativeResult<RootedValue> {
        if !self.is_variant(index, variant)? {
            return Err(invalid());
        }
        let source = self.runtime.checked_enum_variant(
            &self.contract.owner,
            self.input_type(index)?,
            variant,
        )?;
        let destination =
            self.runtime
                .checked_enum_variant(&self.contract.owner, target, variant)?;
        let original = self.output(index)?.ok_or_else(invalid)?;
        let Value::Enum(id) = original else {
            return Err(invalid());
        };
        let snapshot = self.runtime.gc().enum_snapshot(id).ok_or_else(invalid)?;
        // Checked allocation below validates every payload against the target's
        // pinned layout. Variant identity checks above exclude unrelated carriers.
        if source.variant().payload.len() != destination.variant().payload.len() {
            return Err(invalid());
        }
        for index in 0..source.variant().payload.len() {
            let (source_ty, source_environment) = source.payload_type(index).ok_or_else(invalid)?;
            let (target_ty, target_environment) =
                destination.payload_type(index).ok_or_else(invalid)?;
            if !TypeView::new(source_ty, source.module(), source_environment).compatible(
                TypeView::new(target_ty, destination.module(), target_environment),
            ) {
                return Err(invalid());
            }
        }
        let origin = self.runtime.gc().result_error_trace(&original);
        let value = self
            .runtime
            .gc()
            .alloc_enum_origin(EnumTag::Declared(destination), snapshot.fields, || origin)
            .map(Value::Enum)?;
        self.root_checked(target, value)
    }
}

impl CallContext<'_> {
    /// Construct a cold two-input composition using the linked native signature.
    /// No branch is claimed or started until the returned Future is driven.
    pub fn compose_pair(&self, policy: Arc<dyn PairPolicy>) -> NativeResult<Value> {
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
            if !ty.matches(self.runtime, value, self.owner) {
                return Err(invalid());
            }
        }
        self.runtime.gc().validate_async_values(&values)?;
        let cold = ColdComposition {
            owner: self.owner.clone(),
            inputs,
            output,
            values,
            policy,
        };
        cold.policy.validate(&PairContext {
            runtime: self.runtime,
            contract: &cold,
            outputs: &[None, None],
        })?;
        self.allocate_result_payload(&[], |_, _| {
            Ok(FuturePayload {
                cold: Some(ColdFuture::Composition(cold)),
            })
        })
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
