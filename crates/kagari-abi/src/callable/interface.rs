//! Applied interface method signatures retain separate receiver and method binders.
use crate::callable::witness::{OperationWitness, required_operations};
use crate::declaration::ModuleDecl;
use crate::native_import::callables::NativeCallableRequirement;
use crate::representation::ValueType;
use crate::types::{
    AbiType, ConstraintAbi, GenericBoundAbi, GenericParameterAbi, NominalAbiType, PublicAbiItem,
    TraitAbi, TraitContract,
    proofs::ProofCatalog,
    substitution::{TypeSubstitution, TypeTransformError, resolve_associated_outputs},
    trait_contract,
    verify::types_in_scope,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, ModuleIdentity},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterfaceCallContract {
    /// None dispatches a boxed interface; Some invokes the caller's checked
    /// constraint operation on an unboxed value of this type.
    pub receiver: Option<AbiType>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub operations: Vec<OperationWitness>,
    pub interface: NominalAbiType,
    pub method_slot: u32,
    /// Arguments for this method only. The interface owns its trait arguments.
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub arguments: Vec<AbiType>,
}

pub struct InterfaceMethodSignature {
    pub params: Vec<AbiType>,
    pub result: AbiType,
    pub bounds: Vec<GenericBoundAbi>,
}

impl InterfaceMethodSignature {
    pub fn types_valid(
        &self,
        parameters: &[GenericParameterAbi],
        cancel: &CancellationToken,
    ) -> bool {
        types_in_scope(self.params.iter().chain([&self.result]), parameters, cancel)
    }

    pub fn physical_types(
        &self,
        parameters: &[GenericParameterAbi],
        cancel: &CancellationToken,
    ) -> Option<(Vec<ValueType>, ValueType)> {
        self.types_valid(parameters, cancel).then(|| {
            (
                self.params.iter().map(AbiType::representation).collect(),
                self.result.representation(),
            )
        })
    }
}

impl InterfaceCallContract {
    pub fn signature_in(
        &self,
        owner: &ModuleIdentity,
        items: &[PublicAbiItem],
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
        contract: &TraitAbi,
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
            || method.params.first().is_none_or(|parameter| {
                parameter.ty != AbiType::SelfType(interface.declaration.clone())
            })
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
        let apply = |ty: &AbiType| {
            resolve_associated_outputs(&substitution.apply(ty, cancel)?, interface, cancel)
        };
        let mut params = vec![
            self.receiver
                .clone()
                .unwrap_or_else(|| AbiType::Trait(interface.clone())),
        ];
        for parameter in method.params.iter().skip(1) {
            params.push(apply(&parameter.ty)?);
        }
        let mut bounds = substitution.apply_bounds(&contract.bounds, cancel)?;
        bounds.extend(substitution.apply_bounds(&method.bounds, cancel)?);
        for bound in &mut bounds {
            bound.ty = resolve_associated_outputs(&bound.ty, interface, cancel)?;
            for constraint in &mut bound.constraints {
                if let ConstraintAbi::Trait(required) = constraint {
                    let AbiType::Trait(resolved) = resolve_associated_outputs(
                        &AbiType::Trait(required.clone()),
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
        contract: &TraitAbi,
        catalog: &ProofCatalog<'_>,
        parameters: &[GenericParameterAbi],
        assumptions: &[GenericBoundAbi],
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
                .map(|required| required.normalized(catalog, cancel))
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
