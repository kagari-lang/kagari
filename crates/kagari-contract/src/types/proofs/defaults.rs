//! Check default applications against actual registered templates and trait facts.
use kagari_common::cancellation::CancellationToken;

use crate::{
    native_import::NativeSignature,
    types::{
        ConcreteFunctionIdentity,
        proofs::{Budget, ProofCatalog},
    },
};
use kagari_types::{
    callable::{CallableImplementation, NativeDefaultApplication},
    declaration::verify::types_in_scope,
    ty::{
        Constraint, GenericBound, GenericParam, NominalTy, Ty,
        substitution::{TypeSubstitution, TypeTransformError},
    },
};
use std::collections::BTreeMap;

pub struct ResolvedNativeDefault {
    pub instance: ConcreteFunctionIdentity,
    pub implementation: CallableImplementation,
    pub signature: NativeSignature,
}

impl ProofCatalog<'_> {
    pub(super) fn native_defaults_valid(
        &self,
        cancel: &CancellationToken,
    ) -> Result<bool, TypeTransformError> {
        let budget = Budget::new(cancel);
        for (owner, contract) in &self.contracts {
            for method in &contract.methods {
                budget.step(0)?;
                let CallableImplementation::NativeDefault(application) = &method.implementation
                else {
                    continue;
                };
                let Some(template) = self.native_declarations.get(&application.declaration) else {
                    return Ok(false);
                };
                if !matches!(
                    template.function.implementation,
                    CallableImplementation::Native(_)
                ) || application.arguments.len() != template.function.generic_params.len()
                    || method.params.len() != template.function.params.len()
                {
                    return Ok(false);
                }
                let mut substitution = TypeSubstitution::default();
                for (parameter, argument) in template
                    .function
                    .generic_params
                    .iter()
                    .zip(&application.arguments)
                {
                    substitution.bind(&parameter.owner, parameter.position, argument);
                }
                if substitution.apply(&template.function.return_type, cancel)? != method.return_type
                {
                    return Ok(false);
                }
                for (actual, expected) in template.function.params.iter().zip(&method.params) {
                    if substitution.apply(&actual.ty, cancel)? != expected.ty
                        || actual.mutable != expected.mutable
                    {
                        return Ok(false);
                    }
                }
                let mut assumptions = contract.bounds.clone();
                assumptions.extend(method.bounds.clone());
                let mut interface = NominalTy {
                    declaration: (*owner).clone(),
                    arguments: contract
                        .generic_params
                        .iter()
                        .map(|p| p.as_type())
                        .collect(),
                    associated_types: BTreeMap::new(),
                };
                let base = interface.clone();
                for output in &contract.associated_types {
                    budget.step(0)?;
                    if !output.generic_params.is_empty() {
                        continue;
                    }
                    let projection = Ty::Projection {
                        receiver: Box::new(Ty::SelfType((*owner).clone())),
                        interface: Box::new(base.clone()),
                        member: output.declaration.clone(),
                        arguments: vec![],
                    };
                    interface
                        .associated_types
                        .insert(output.declaration.clone(), projection.clone());
                    assumptions.push(GenericBound {
                        ty: projection,
                        constraints: output.bounds.clone(),
                    });
                }
                assumptions.push(GenericBound {
                    ty: Ty::SelfType((*owner).clone()),
                    constraints: vec![Constraint::Trait(interface)],
                });
                for bound in substitution.apply_bounds(&template.function.bounds, cancel)? {
                    budget.step(0)?;
                    if !self.constraints_hold(
                        &bound.ty,
                        &bound.constraints,
                        &assumptions,
                        cancel,
                    )? {
                        return Ok(false);
                    }
                }
            }
        }
        Ok(true)
    }

    /// Resolve an already substituted default application to a normal native
    /// target. The template and its bounds are required; a binding name is never
    /// used to infer the receiver or another type argument.
    pub fn resolve_native_default(
        &self,
        application: &NativeDefaultApplication,
        cancel: &CancellationToken,
    ) -> Result<Option<ResolvedNativeDefault>, TypeTransformError> {
        self.resolve_native_default_in(application, &[], &[], cancel)
    }

    pub fn resolve_native_default_in(
        &self,
        application: &NativeDefaultApplication,
        parameters: &[GenericParam],
        assumptions: &[GenericBound],
        cancel: &CancellationToken,
    ) -> Result<Option<ResolvedNativeDefault>, TypeTransformError> {
        cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
        let Some(template) = self.native_declarations.get(&application.declaration) else {
            return Ok(None);
        };
        if application.arguments.len() != template.function.generic_params.len()
            || !matches!(
                template.function.implementation,
                CallableImplementation::Native(_)
            )
        {
            return Ok(None);
        }
        let arguments = application
            .arguments
            .iter()
            .map(|argument| self.normalize(argument, cancel))
            .collect::<Result<Vec<_>, _>>()?;
        if !types_in_scope(&arguments, parameters, cancel) {
            return Ok(None);
        }
        let mut substitution = TypeSubstitution::default();
        for (parameter, argument) in template.function.generic_params.iter().zip(&arguments) {
            substitution.bind(&parameter.owner, parameter.position, argument);
        }
        for bound in substitution.apply_bounds(&template.function.bounds, cancel)? {
            if !self.constraints_hold(&bound.ty, &bound.constraints, assumptions, cancel)? {
                return Ok(None);
            }
        }
        let normalize = |ty| self.normalize(&substitution.apply(ty, cancel)?, cancel);
        let signature = NativeSignature {
            params: template
                .function
                .params
                .iter()
                .map(|p| normalize(&p.ty))
                .collect::<Result<_, _>>()?,
            result: normalize(&template.function.return_type)?,
        };
        Ok(Some(ResolvedNativeDefault {
            instance: ConcreteFunctionIdentity {
                declaration: application.declaration.clone(),
                arguments,
            },
            implementation: template.function.implementation.clone(),
            signature,
        }))
    }
}
