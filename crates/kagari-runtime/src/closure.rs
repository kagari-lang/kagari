//! Checked closure call metadata, independent of collector implementation.
mod native;
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::{
        TypeEnvironment, arguments::ScopedSignature, bindings::TypeBindings,
        compatibility::TypeView,
    },
    module::LoadedModule,
    value::Value,
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::instruction::NativeImportId;
use kagari_common::identity::table::DefinitionId;
use kagari_contract::{ids::FunctionRef, representation::semantic_representation};
use kagari_types::ty::Ty;
use std::{borrow::Cow, slice, sync::Arc};

/// A checked closure may enter ordinary script code or an installed native entry.
#[derive(Debug, Clone)]
pub enum ClosureTarget {
    Script(FunctionRef),
    Native(NativeClosure),
}

/// Only runtime preparation constructs native closure metadata. Its signature
/// keeps lexical type scopes; executable retention uses the enclosing graph edges.
#[derive(Debug, Clone)]
pub struct NativeClosure {
    pub(crate) import: NativeImportId,
    pub(crate) signature: Arc<ScopedSignature>,
}

/// Closure metadata stored by value in the runtime heap. An explicit diagnostic
/// copy does not root captures or authorize execution after its source is collected.
#[derive(Debug, Clone)]
pub struct ClosureValueSnapshot {
    pub environment: Option<TypeEnvironment>,
    pub implementation: LoadedModule,
    pub target: ClosureTarget,
    pub captures: Vec<Value>,
}

impl ClosureValueSnapshot {
    pub fn script_function(&self) -> Option<FunctionRef> {
        match self.target {
            ClosureTarget::Script(function) => Some(function),
            ClosureTarget::Native(_) => None,
        }
    }

    pub(crate) fn signature(
        &self,
        runtime: &Runtime,
    ) -> Result<Arc<ScopedSignature>, RuntimeError> {
        let function = match &self.target {
            ClosureTarget::Native(native) => return Ok(native.signature.clone()),
            ClosureTarget::Script(function) => function,
        };
        let metadata = &self
            .implementation
            .bytecode
            .functions
            .get(function.index())
            .ok_or_else(|| RuntimeError::module_validation("closure function"))?
            .metadata;
        let types = (self.captures.len()..metadata.params.len())
            .map(|index| {
                metadata
                    .semantic
                    .params
                    .get(&index)
                    .cloned()
                    .ok_or_else(|| RuntimeError::module_validation("closure parameter type"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let environment = self
            .environment
            .as_ref()
            .map(|environment| environment.types.clone());
        let params = runtime.type_arguments(&self.implementation, environment.clone(), &types)?;
        let result = metadata
            .semantic
            .result
            .as_ref()
            .ok_or_else(|| RuntimeError::module_validation("closure result type"))?;
        let result = runtime
            .type_arguments(&self.implementation, environment, slice::from_ref(result))?
            .pop()
            .ok_or_else(|| RuntimeError::module_validation("closure result scope"))?;
        Ok(Arc::new(ScopedSignature { params, result }))
    }

    pub fn physical_signature(&self) -> Result<(Cow<'_, [ValueType]>, ValueType), RuntimeError> {
        let function = match &self.target {
            ClosureTarget::Native(native) => {
                return Ok((
                    Cow::Owned(
                        native
                            .signature
                            .params
                            .iter()
                            .map(|ty| semantic_representation(ty.ty()))
                            .collect(),
                    ),
                    semantic_representation(native.signature.result.ty()),
                ));
            }
            ClosureTarget::Script(function) => function,
        };
        let function = self
            .implementation
            .bytecode
            .functions
            .get(function.index())
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
        let function = match &self.target {
            ClosureTarget::Native(native) => {
                return native.signature.params.len() == params.len()
                    && native
                        .signature
                        .params
                        .iter()
                        .zip(params)
                        .all(|(actual, expected)| {
                            actual.view(&self.implementation).compatible(TypeView::new(
                                expected,
                                owner,
                                environment,
                            ))
                        })
                    && native
                        .signature
                        .result
                        .view(&self.implementation)
                        .compatible(TypeView::new(result, owner, environment));
            }
            ClosureTarget::Script(function) => function,
        };
        let Some(function) = self.implementation.bytecode.functions.get(function.index()) else {
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
