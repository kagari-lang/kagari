//! Checked closure call metadata, independent of collector implementation.
use crate::{
    error::RuntimeError,
    frame::types::{TypeEnvironment, bindings::TypeBindings, compatibility::TypeView},
    module::LoadedModule,
    value::Value,
};
use kagari_abi::representation::ValueType;
use kagari_common::identity::table::DefinitionId;
use kagari_contract::{ids::FunctionRef, representation::semantic_representation};
use kagari_types::ty::Ty;
use std::borrow::Cow;

/// Closure metadata stored by value in the runtime heap. An explicit diagnostic
/// copy does not root captures or authorize execution after its source is collected.
#[derive(Debug, Clone)]
pub struct ClosureValueSnapshot {
    pub environment: Option<TypeEnvironment>,
    pub implementation: LoadedModule,
    pub function: FunctionRef,
    pub captures: Vec<Value>,
}

impl ClosureValueSnapshot {
    pub fn physical_signature(&self) -> Result<(Cow<'_, [ValueType]>, ValueType), RuntimeError> {
        let function = self
            .implementation
            .bytecode
            .functions
            .get(self.function.index())
            .ok_or_else(|| RuntimeError::module_validation("closure function"))?;
        let parameters = function
            .metadata
            .params
            .get(self.captures.len()..)
            .ok_or_else(|| RuntimeError::module_validation("closure captures"))?;
        if function.metadata.return_type != ValueType::Generic
            && !parameters.contains(&ValueType::Generic)
        {
            return Ok((Cow::Borrowed(parameters), function.metadata.return_type));
        }
        let resolve = |physical, semantic: Option<&Ty<DefinitionId>>| {
            if physical != ValueType::Generic {
                return Ok(physical);
            }
            let ty =
                semantic.ok_or_else(|| RuntimeError::module_validation("generic closure type"))?;
            let environment = self
                .environment
                .as_ref()
                .ok_or_else(|| RuntimeError::module_validation("generic closure environment"))?;
            Ok(semantic_representation(&environment.types.resolve(ty)?))
        };
        let params = function
            .metadata
            .params
            .iter()
            .enumerate()
            .skip(self.captures.len())
            .map(|(index, ty)| resolve(*ty, function.metadata.semantic.params.get(&index)))
            .collect::<Result<_, _>>()?;
        Ok((
            Cow::Owned(params),
            resolve(
                function.metadata.return_type,
                function.metadata.semantic.result.as_ref(),
            )?,
        ))
    }

    pub(crate) fn matches_function(
        &self,
        params: &[Ty<DefinitionId>],
        result: &Ty<DefinitionId>,
        owner: &LoadedModule,
        environment: Option<&TypeBindings>,
    ) -> bool {
        let Some(function) = self
            .implementation
            .bytecode
            .functions
            .get(self.function.index())
        else {
            return false;
        };
        let compatible = |actual: &Ty<DefinitionId>, expected: &Ty<DefinitionId>| {
            TypeView::new(
                actual,
                &self.implementation,
                self.environment
                    .as_ref()
                    .map(|environment| environment.types.as_ref()),
            )
            .compatible(TypeView::new(expected, owner, environment))
        };
        let captures = self.captures.len();
        function
            .metadata
            .params
            .get(captures..)
            .is_some_and(|suffix| suffix.len() == params.len())
            && params.iter().enumerate().all(|(index, expected)| {
                function
                    .metadata
                    .semantic
                    .params
                    .get(&(captures + index))
                    .is_some_and(|actual| compatible(actual, expected))
            })
            && function
                .metadata
                .semantic
                .result
                .as_ref()
                .is_some_and(|actual| compatible(actual, result))
    }
}
