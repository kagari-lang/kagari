//! Recheck a concrete trait-member selection against carried implementation facts.
use crate::{
    callable::CallableImplementation,
    effects::EffectSet,
    native_import::{
        NativeSignature,
        callables::{NativeCallableApplication, NativeCallableOrigin, NativeCallableRequirement},
    },
    types::{
        ConcreteFunctionIdentity, matching,
        proofs::ProofCatalog,
        substitution::{TypeSubstitution, TypeTransformError},
    },
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, DefinitionPathSegment},
};

impl ProofCatalog<'_> {
    pub fn callable_requirement_valid(&self, requirement: &NativeCallableRequirement) -> bool {
        self.contracts
            .get(&requirement.interface.declaration)
            .is_some_and(|contract| {
                contract.generic_params.len() == requirement.interface.arguments.len()
                    && contract.methods.iter().any(|method| {
                        requirement
                            .member
                            .path
                            .last()
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
        let mut parent = requirement.member.clone();
        let Some(member) = parent.path.pop() else {
            return Ok(None);
        };
        if parent != requirement.interface.declaration
            || member.kind != DefinitionKind::Method
            || member.occurrence != 0
            || !self.callable_requirement_valid(requirement)
            || !self.holds(&requirement.interface, &requirement.receiver, &[], cancel)?
        {
            return Ok(None);
        }
        let mut selected = None;
        for table in &self.implementations {
            cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
            let Some(bindings) = matching::match_pattern(
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
            let obligations = bindings.apply_bounds(table.bounds(), cancel)?;
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
                &member.name,
                &arguments,
                self.contracts
                    .get(&requirement.interface.declaration)
                    .copied(),
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
            for bound in substitution.apply_bounds(&method.bounds, cancel)? {
                if !self.constraints_hold(&bound.ty, &bound.constraints, &[], cancel)? {
                    return Ok(None);
                }
            }
            let normalize = |ty| self.normalize(&substitution.apply(ty, cancel)?, cancel);
            let mut declaration = table.declaration().clone();
            declaration.path.push(DefinitionPathSegment {
                kind: DefinitionKind::Method,
                name: member.name.clone(),
                occurrence: 0,
            });
            let signature = NativeSignature {
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
        Ok(selected)
    }
}
