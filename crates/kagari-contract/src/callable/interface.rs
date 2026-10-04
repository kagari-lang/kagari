//! Applied interface method signatures retain separate receiver and method binders.
use crate::callable::witness::{OperationWitness, required_operations};
use crate::{
    native_import::callables::normalize_requirement,
    representation::semantic_representation,
    types::{PublicItem, TraitContract, proofs::ProofCatalog, trait_contract},
};
use kagari_abi::representation::ValueType;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, DefinitionPath, ModuleIdentity, reference::DefinitionReference},
};
use kagari_types::{
    declaration::{
        TraitDef, module::ModuleDecl, requirement::NativeCallableRequirement,
        verify::types_in_scope,
    },
    ty::{
        Constraint, GenericBound, GenericParam, NominalTy, Ty,
        substitution::{TypeSubstitution, TypeTransformError, resolve_associated_outputs},
    },
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct InterfaceCallContract<I = DefinitionPath> {
    /// None dispatches a boxed interface; Some invokes the caller's checked
    /// constraint operation on an unboxed value of this type.
    pub receiver: Option<Ty<I>>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub operations: Vec<OperationWitness<I>>,
    pub interface: NominalTy<I>,
    pub method_slot: u32,
    /// Arguments for this method only. The interface owns its trait arguments.
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub arguments: Vec<Ty<I>>,
}

pub struct InterfaceMethodSignature<I = DefinitionPath> {
    pub params: Vec<Ty<I>>,
    pub result: Ty<I>,
    pub bounds: Vec<GenericBound<I>>,
}

impl InterfaceMethodSignature {
    pub fn types_valid(&self, parameters: &[GenericParam], cancel: &CancellationToken) -> bool {
        types_in_scope(self.params.iter().chain([&self.result]), parameters, cancel)
    }

    pub fn physical_types(
        &self,
        parameters: &[GenericParam],
        cancel: &CancellationToken,
    ) -> Option<(Vec<ValueType>, ValueType)> {
        self.types_valid(parameters, cancel).then(|| {
            (
                self.params.iter().map(semantic_representation).collect(),
                semantic_representation(&self.result),
            )
        })
    }
}

impl InterfaceCallContract {
    pub fn signature_in(
        &self,
        owner: &ModuleIdentity,
        items: &[PublicItem],
        contracts: &[TraitContract],
        cancel: &CancellationToken,
    ) -> Result<InterfaceMethodSignature, TypeTransformError> {
        if self.interface.declaration.module != *owner {
            return Err(TypeTransformError::InvalidContract);
        }
        let contract = trait_contract(owner, items, contracts, &self.interface.declaration)
            .ok_or(TypeTransformError::InvalidContract)?;
        self.signature(contract, cancel)
    }

    /// Apply a validated declaration without resolving any implementation. A
    /// shared caller may forward its own parameters; check() verifies their scope
    /// and required bounds independently of this substitution.
    pub fn signature(
        &self,
        contract: &TraitDef,
        cancel: &CancellationToken,
    ) -> Result<InterfaceMethodSignature, TypeTransformError> {
        cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
        let invalid = TypeTransformError::InvalidContract;
        let interface = &self.interface;
        let path = &interface.declaration.path;
        if path.len() != 1
            || path[0].kind != DefinitionKind::Trait
            || path[0].occurrence != 0
            || path[0].name != contract.name
            || interface.arguments.len() != contract.generic_params.len()
            || !contract.associated_consts.is_empty()
            || contract
                .associated_types
                .iter()
                .any(|member| !member.generic_params.is_empty())
            || interface.associated_types.len() != contract.associated_types.len()
            || contract
                .associated_types
                .iter()
                .any(|member| !interface.associated_types.contains_key(&member.declaration))
        {
            return Err(invalid);
        }
        let method = contract
            .methods
            .get(self.method_slot as usize)
            .ok_or(invalid)?;
        if method.generic_params.len() != self.arguments.len()
            || method
                .params
                .first()
                .is_none_or(|parameter| parameter.ty != Ty::SelfType(interface.declaration.clone()))
        {
            return Err(invalid);
        }
        let mut substitution = TypeSubstitution::default();
        for (parameter, argument) in contract
            .generic_params
            .iter()
            .zip(&interface.arguments)
            .chain(method.generic_params.iter().zip(&self.arguments))
        {
            substitution.bind(&parameter.owner, parameter.position, argument);
        }
        if let Some(receiver) = &self.receiver {
            substitution.bind_receiver(&interface.declaration, receiver);
        }
        let apply = |ty: &Ty| {
            resolve_associated_outputs(&substitution.apply(ty, cancel)?, interface, cancel)
        };
        let mut params = vec![
            self.receiver
                .clone()
                .unwrap_or_else(|| Ty::Trait(interface.clone())),
        ];
        for parameter in method.params.iter().skip(1) {
            params.push(apply(&parameter.ty)?);
        }
        let mut bounds = substitution.apply_bounds(&contract.bounds, cancel)?;
        bounds.extend(substitution.apply_bounds(&method.bounds, cancel)?);
        for bound in &mut bounds {
            bound.ty = resolve_associated_outputs(&bound.ty, interface, cancel)?;
            for constraint in &mut bound.constraints {
                if let Constraint::Trait(required) = constraint {
                    let Ty::Trait(resolved) = resolve_associated_outputs(
                        &Ty::Trait(required.clone()),
                        interface,
                        cancel,
                    )?
                    else {
                        return Err(invalid);
                    };
                    *required = resolved;
                }
            }
        }
        Ok(InterfaceMethodSignature {
            params,
            result: apply(&method.return_type)?,
            bounds,
        })
    }

    /// The caller's scope and assumptions come from its verified body contract.
    /// A type argument is never accepted merely because it has a slot layout.
    pub fn check(
        &self,
        contract: &TraitDef,
        catalog: &ProofCatalog<'_>,
        parameters: &[GenericParam],
        assumptions: &[GenericBound],
        cancel: &CancellationToken,
    ) -> Result<bool, TypeTransformError> {
        let signature = self.signature(contract, cancel)?;
        if !types_in_scope(
            self.arguments
                .iter()
                .chain(signature.params.iter())
                .chain([&signature.result]),
            parameters,
            cancel,
        ) {
            return Ok(false);
        }
        for bound in &signature.bounds {
            if !catalog.constraints_hold(&bound.ty, &bound.constraints, assumptions, cancel)? {
                return Ok(false);
            }
        }
        if let Some(receiver) = &self.receiver
            && !catalog.holds(&self.interface, receiver, assumptions, cancel)?
        {
            return Ok(false);
        }
        let expected =
            required_operations(&signature.bounds, &|id| catalog.trait_contract(id), cancel)?
                .iter()
                .map(|required| normalize_requirement(required, catalog, cancel))
                .collect::<Result<Vec<_>, _>>()?;
        if expected.len() != self.operations.len()
            || expected.iter().any(|required| {
                self.operations
                    .iter()
                    .filter(|operation| operation.requirement() == required)
                    .count()
                    != 1
            })
        {
            return Ok(false);
        }
        if let Some(receiver) = &self.receiver {
            let required = NativeCallableRequirement {
                receiver: receiver.clone(),
                interface: self.interface.clone(),
                member: ModuleDecl::method_id(
                    &self.interface.declaration,
                    &contract.methods[self.method_slot as usize].name,
                ),
                arguments: vec![],
            };
            let available =
                required_operations(assumptions, &|id| catalog.trait_contract(id), cancel)?;
            if !available.contains(&required) {
                return Ok(false);
            }
        }
        for operation in &self.operations {
            if !operation.valid(catalog, parameters, assumptions, cancel)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests;

mod mapping;
