//! A statically selected shared entry with caller-scoped type and operation arguments.
use crate::{
    callable::{
        CallableImplementation,
        generic::GenericBody,
        witness::{OperationWitness, required_operations},
    },
    native_import::NativeSignature,
    types::{
        AbiType, ConcreteFunctionIdentity, GenericBoundAbi, GenericParameterAbi,
        proofs::ProofCatalog,
        substitution::{TypeSubstitution, TypeTransformError},
        verify::types_in_scope,
    },
};
use kagari_common::cancellation::CancellationToken;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedCall {
    pub instance: ConcreteFunctionIdentity,
    pub implementation: CallableImplementation,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub arguments: Vec<AbiType>,
    pub signature: NativeSignature,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub operations: Vec<OperationWitness>,
}

impl SharedCall {
    pub fn structurally_valid(
        &self,
        parameters: &[GenericParameterAbi],
        cancel: &CancellationToken,
    ) -> bool {
        self.instance.declaration.within_path_limit()
            && self.instance.arguments.len() <= 4096
            && self
                .instance
                .arguments
                .iter()
                .all(AbiType::within_wire_limits)
            && !self.arguments.is_empty()
            && self.arguments.len() <= 4096
            && self.operations.len() <= 4096
            && self.signature.params.len() <= 4096
            && matches!(
                self.implementation,
                CallableImplementation::Script | CallableImplementation::Native(_)
            )
            && types_in_scope(
                self.arguments
                    .iter()
                    .chain(&self.signature.params)
                    .chain([&self.signature.result]),
                parameters,
                cancel,
            )
            && self
                .operations
                .iter()
                .all(|operation| operation.structurally_valid(parameters))
    }

    pub fn check(
        &self,
        body: &GenericBody,
        signature: &NativeSignature,
        catalog: &ProofCatalog<'_>,
        parameters: &[GenericParameterAbi],
        assumptions: &[GenericBoundAbi],
        cancel: &CancellationToken,
    ) -> Result<bool, TypeTransformError> {
        if !self.structurally_valid(parameters, cancel)
            || !body.valid(&self.instance, cancel)
            || body.parameters.len() != self.arguments.len()
        {
            return Ok(false);
        }
        let substitution = self.substitution(body);
        let normalize = |ty| catalog.normalize(&substitution.apply(ty, cancel)?, cancel);
        if signature
            .params
            .iter()
            .map(normalize)
            .collect::<Result<Vec<_>, _>>()?
            != self.signature.params
            || normalize(&signature.result)? != self.signature.result
        {
            return Ok(false);
        }
        let bounds = substitution.apply_bounds(&body.bounds, cancel)?;
        for bound in &bounds {
            if !catalog.constraints_hold(&bound.ty, &bound.constraints, assumptions, cancel)? {
                return Ok(false);
            }
        }
        let required = required_operations(&bounds, &|id| catalog.trait_contract(id), cancel)?
            .iter()
            .map(|required| required.normalized(catalog, cancel))
            .collect::<Result<Vec<_>, _>>()?;
        if required.len() != self.operations.len()
            || required.iter().any(|required| {
                self.operations
                    .iter()
                    .filter(|operation| operation.requirement() == required)
                    .count()
                    != 1
            })
        {
            return Ok(false);
        }
        for operation in &self.operations {
            if !operation.valid(catalog, parameters, assumptions, cancel)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn substitution<'a>(&'a self, body: &'a GenericBody) -> TypeSubstitution<'a> {
        let mut substitution = TypeSubstitution::default();
        for (parameter, argument) in body.parameters.iter().zip(&self.arguments) {
            substitution.bind(&parameter.owner, parameter.position, argument);
        }
        substitution
    }
}
