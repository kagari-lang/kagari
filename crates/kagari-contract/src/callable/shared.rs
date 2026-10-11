//! A statically selected shared entry with caller-scoped type and operation arguments.
use crate::native_import::callables::normalize_requirement;
use crate::{
    callable::{
        generic::GenericBody,
        witness::{OperationWitness, required_operations},
    },
    types::{ConcreteFunctionIdentity, proofs::ProofCatalog},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::table::{DefinitionId, DefinitionTable},
};
use kagari_types::{
    callable::{CallableImplementation, Signature},
    declaration::verify::types_in_scope,
    ty::{
        GenericBound, GenericParam, Ty,
        substitution::{TypeSubstitution, TypeTransformError},
    },
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SharedCall {
    pub instance: ConcreteFunctionIdentity,
    pub implementation: CallableImplementation<DefinitionId>,
    pub arguments: Vec<Ty<DefinitionId>>,
    pub signature: Signature<DefinitionId>,
    pub operations: Vec<OperationWitness>,
}

impl SharedCall {
    pub fn structurally_valid(
        &self,
        parameters: &[GenericParam<DefinitionId>],
        cancel: &CancellationToken,
        definitions: &DefinitionTable,
    ) -> bool {
        definitions.resolve(self.instance.declaration).is_ok()
            && self.instance.arguments.len() <= 4096
            && self.instance.arguments.iter().all(Ty::within_wire_limits)
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
                definitions,
            )
            && self
                .operations
                .iter()
                .all(|operation| operation.structurally_valid(parameters, definitions))
    }

    pub fn check(
        &self,
        body: &GenericBody,
        signature: &Signature<DefinitionId>,
        catalog: &ProofCatalog<'_>,
        parameters: &[GenericParam<DefinitionId>],
        assumptions: &[GenericBound<DefinitionId>],
        cancel: &CancellationToken,
    ) -> Result<bool, TypeTransformError> {
        let definitions = catalog.definitions();
        if !self.structurally_valid(parameters, cancel, definitions)
            || !body.valid(&self.instance, cancel, definitions)
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
        let bounds = substitution.apply_bounds(&body.bounds, catalog.definitions(), cancel)?;
        for bound in &bounds {
            if !catalog.constraints_hold(&bound.ty, &bound.constraints, assumptions, cancel)? {
                return Ok(false);
            }
        }
        let required = required_operations(
            &bounds,
            &|id| catalog.trait_contract(id),
            cancel,
            definitions,
        )?
        .iter()
        .map(|required| normalize_requirement(required, catalog, cancel))
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

mod mapping;
