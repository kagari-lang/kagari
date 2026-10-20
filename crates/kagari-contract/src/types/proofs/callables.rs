//! Recheck a concrete trait-member selection against carried implementation facts.
use crate::{
    callable::witness::{
        SharedMethodWitness, is_generic_member, required_operations, requirement_signature_in_scope,
    },
    effects::EffectSet,
    native_import::{
        callables::{NativeCallableApplication, NativeCallableOrigin, normalize_requirement},
        result::NativeResultAdapter,
    },
    types::{
        ConcreteFunctionIdentity,
        proofs::{ProofCatalog, satisfies},
    },
};
use kagari_common::{cancellation::CancellationToken, identity::DefinitionKind};
use kagari_types::{
    callable::{CallableImplementation, Signature},
    declaration::requirement::NativeCallableRequirement,
    ty::{
        GenericBound, GenericParam, Ty,
        matching::match_pattern,
        substitution::{TypeSubstitution, TypeTransformError, resolve_associated_outputs},
    },
};

impl ProofCatalog<'_> {
    pub fn native_result_matches(
        &self,
        selected: &NativeResultAdapter,
        result: &Ty,
        assumptions: &[GenericBound],
        cancel: &CancellationToken,
    ) -> Result<bool, TypeTransformError> {
        let Some(table) = self
            .implementations
            .iter()
            .find(|table| table.declaration() == &selected.implementation.declaration)
        else {
            return Ok(false);
        };
        let Some(interface) = table.interface() else {
            return Ok(false);
        };
        if table.parameters().len() != selected.implementation.arguments.len() {
            return Ok(false);
        }
        let mut substitution = TypeSubstitution::default();
        for (parameter, argument) in table
            .parameters()
            .iter()
            .zip(&selected.implementation.arguments)
        {
            substitution.bind(&parameter.owner, parameter.position, argument);
        }
        if self.normalize(&substitution.apply(table.receiver(), cancel)?, cancel)?
            != selected.receiver
            || self.normalize(
                &Ty::Trait(substitution.apply_nominal(interface, cancel)?),
                cancel,
            )? != *result
        {
            return Ok(false);
        }
        for bound in substitution.apply_bounds(table.bounds(), &self.definitions, cancel)? {
            if !self.constraints_hold(&bound.ty, &bound.constraints, assumptions, cancel)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub fn shared_method_matches(
        &self,
        selected: &SharedMethodWitness,
        parameters: &[GenericParam],
        assumptions: &[GenericBound],
        cancel: &CancellationToken,
    ) -> Result<bool, TypeTransformError> {
        let required = &selected.requirement;
        if !is_generic_member(required, self)
            && requirement_signature_in_scope(required, self, parameters, assumptions, cancel)?
                .is_none()
        {
            return Ok(false);
        }
        let Some(table) = self
            .implementations
            .iter()
            .find(|table| table.declaration() == &selected.implementation.declaration)
        else {
            return Ok(false);
        };
        if table.parameters().len() != selected.implementation.arguments.len() {
            return Ok(false);
        }
        let mut substitution = TypeSubstitution::default();
        for (parameter, argument) in table
            .parameters()
            .iter()
            .zip(&selected.implementation.arguments)
        {
            substitution.bind(&parameter.owner, parameter.position, argument);
        }
        let Some(interface) = table.interface() else {
            return Ok(false);
        };
        if !satisfies(
            &substitution.apply_nominal(interface, cancel)?,
            &required.interface,
        ) || substitution.apply(table.receiver(), cancel)? != required.receiver
        {
            return Ok(false);
        }
        let bounds = substitution.apply_bounds(table.bounds(), &self.definitions, cancel)?;
        for bound in &bounds {
            if !self.constraints_hold(&bound.ty, &bound.constraints, assumptions, cancel)? {
                return Ok(false);
            }
        }
        let requirements = required_operations(
            &bounds,
            &|id| self.trait_contract(id),
            cancel,
            &self.definitions,
        )?
        .iter()
        .map(|required| normalize_requirement(required, self, cancel))
        .collect::<Result<Vec<_>, _>>()?;
        if requirements.len() != selected.operations.len()
            || requirements.iter().any(|required| {
                selected
                    .operations
                    .iter()
                    .filter(|operation| operation.requirement() == required)
                    .count()
                    != 1
            })
        {
            return Ok(false);
        }
        for operation in &selected.operations {
            if !operation.valid(self, parameters, assumptions, cancel)? {
                return Ok(false);
            }
        }
        let Some(member) = self
            .definitions
            .resolve(required.member)
            .ok()
            .and_then(|view| view.last())
        else {
            return Ok(false);
        };
        Ok(table
            .method_in(
                member.name,
                &selected.implementation.arguments,
                self.trait_contract(&required.interface.declaration),
                parameters,
                &self.definitions,
                cancel,
            )?
            .is_some_and(|method| {
                !matches!(method.implementation, CallableImplementation::Required)
            }))
    }

    pub fn callable_requirement_valid(&self, requirement: &NativeCallableRequirement) -> bool {
        self.contracts
            .get(&requirement.interface.declaration)
            .is_some_and(|contract| {
                contract.generic_params.len() == requirement.interface.arguments.len()
                    && contract.methods.iter().any(|method| {
                        self.definitions
                            .resolve(requirement.member)
                            .ok()
                            .and_then(|view| view.last())
                            .is_some_and(|member| member.name == method.name)
                            && method.generic_params.len() == requirement.arguments.len()
                    })
            })
    }

    pub fn select_callable(
        &self,
        requirement: &NativeCallableRequirement,
        cancel: &CancellationToken,
    ) -> Result<Option<NativeCallableApplication>, TypeTransformError> {
        let Some(member) = self
            .definitions
            .resolve(requirement.member)
            .ok()
            .and_then(|view| view.last())
        else {
            return Ok(None);
        };
        if self.definitions.parent(requirement.member)
            != Ok(Some(requirement.interface.declaration))
            || member.kind != DefinitionKind::Method
            || member.occurrence != 0
            || !self.callable_requirement_valid(requirement)
            || !self.holds(&requirement.interface, &requirement.receiver, &[], cancel)?
        {
            return Ok(None);
        }
        let mut selected = None;
        for table in self.implementations.iter() {
            cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
            let Some(bindings) = match_pattern(
                table.pattern().ok_or(TypeTransformError::InvalidContract)?,
                &requirement.interface,
                &requirement.receiver,
                cancel,
            )?
            else {
                continue;
            };
            let arguments = table
                .parameters()
                .iter()
                .map(|param| bindings.parameter(&param.owner, param.position).cloned())
                .collect::<Option<Vec<_>>>()
                .ok_or(TypeTransformError::InvalidContract)?;
            let obligations = bindings.apply_bounds(table.bounds(), &self.definitions, cancel)?;
            let mut applicable = true;
            for bound in obligations {
                applicable &= self.constraints_hold(&bound.ty, &bound.constraints, &[], cancel)?;
            }
            if !applicable {
                continue;
            }
            if selected.is_some() {
                return Ok(None);
            }
            let Some(method) = table.method(
                member.name,
                &arguments,
                self.contracts
                    .get(&requirement.interface.declaration)
                    .copied(),
                &self.definitions,
                cancel,
            )?
            else {
                return Ok(None);
            };
            if method.generic_params.len() != requirement.arguments.len()
                || matches!(method.implementation, CallableImplementation::Required)
            {
                return Ok(None);
            }
            let mut substitution = TypeSubstitution::default();
            for (param, argument) in method.generic_params.iter().zip(&requirement.arguments) {
                substitution.bind(&param.owner, param.position, argument);
            }
            for bound in substitution.apply_bounds(&method.bounds, &self.definitions, cancel)? {
                if !self.constraints_hold(&bound.ty, &bound.constraints, &[], cancel)? {
                    return Ok(None);
                }
            }
            let normalize = |ty| self.normalize(&substitution.apply(ty, cancel)?, cancel);
            let signature = Signature {
                params: method
                    .params
                    .iter()
                    .map(|param| normalize(&param.ty))
                    .collect::<Result<_, _>>()?,
                result: normalize(&method.return_type)?,
            };
            let (instance, implementation) = if let CallableImplementation::NativeDefault(
                application,
            ) = &method.implementation
            {
                let application = application.apply(&substitution, cancel)?;
                let Some(resolved) = self.resolve_native_default(&application, cancel)? else {
                    return Ok(None);
                };
                if resolved.signature != signature {
                    return Ok(None);
                }
                (resolved.instance, resolved.implementation)
            } else {
                let declaration = self
                    .definitions
                    .lookup_child(*table.declaration(), DefinitionKind::Method, member.name, 0)
                    .ok_or(TypeTransformError::InvalidContract)?;
                (
                    ConcreteFunctionIdentity {
                        declaration,
                        arguments: arguments
                            .into_iter()
                            .chain(requirement.arguments.iter().cloned())
                            .collect(),
                    },
                    method.implementation.clone(),
                )
            };
            selected = Some(NativeCallableApplication {
                origin: NativeCallableOrigin::Implementation,
                requirement: requirement.clone(),
                instance,
                implementation,
                signature,
                effects: EffectSet::native_call(),
            });
        }
        if selected.is_none()
            && !self.has_explicit_implementation(
                &requirement.interface,
                &requirement.receiver,
                cancel,
            )?
        {
            let Some(contract) = self.contracts.get(&requirement.interface.declaration) else {
                return Ok(None);
            };
            let Some(method) = contract
                .methods
                .iter()
                .find(|method| method.name == member.name)
            else {
                return Ok(None);
            };
            if let CallableImplementation::NativeDefault(application) = &method.implementation {
                let mut substitution = TypeSubstitution::default();
                substitution
                    .bind_receiver(&requirement.interface.declaration, &requirement.receiver);
                for (parameter, argument) in contract
                    .generic_params
                    .iter()
                    .zip(&requirement.interface.arguments)
                    .chain(method.generic_params.iter().zip(&requirement.arguments))
                {
                    substitution.bind(&parameter.owner, parameter.position, argument);
                }
                let mut application = application.apply(&substitution, cancel)?;
                application.arguments = application
                    .arguments
                    .iter()
                    .map(|ty| {
                        resolve_associated_outputs(
                            ty,
                            &requirement.interface,
                            &requirement.receiver,
                            cancel,
                        )
                    })
                    .collect::<Result<_, _>>()?;
                let Some(resolved) = self.resolve_native_default(&application, cancel)? else {
                    return Ok(None);
                };
                selected = Some(NativeCallableApplication {
                    origin: NativeCallableOrigin::Implementation,
                    requirement: requirement.clone(),
                    instance: resolved.instance,
                    implementation: resolved.implementation,
                    signature: resolved.signature,
                    effects: EffectSet::native_call(),
                });
            }
        }
        Ok(selected)
    }
}
