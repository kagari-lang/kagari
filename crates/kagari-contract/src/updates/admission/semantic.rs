//! Reuse the existing linked callable/witness validators; never infer source types.
use crate::{
    callable::generic::GenericBody,
    native_import::NativeImport,
    types::{ConcreteFunctionIdentity, proofs::ProofCatalog},
    updates::{
        BodyRecord, CallRequirement, UpdateEvidence,
        limits::{Budget, UpdateError},
    },
};
use kagari_types::{
    callable::CallableImplementation,
    declaration::verify::types_in_scope,
    ty::{GenericBound, GenericParam, Ty},
};
use std::collections::HashMap;

pub(super) fn validate(
    evidence: &UpdateEvidence,
    catalog: &ProofCatalog<'_>,
    budget: &mut Budget<'_>,
) -> Result<(), UpdateError> {
    let mut bodies = HashMap::new();
    for body in &evidence.bodies {
        budget.visit()?;
        if bodies.insert(&body.requirement.origin, body).is_some() {
            return Err(UpdateError::InvalidEvidence);
        }
    }
    let mut natives: HashMap<_, Vec<_>> = HashMap::new();
    for call in evidence
        .selections
        .iter()
        .chain(
            evidence
                .bodies
                .iter()
                .flat_map(|body| &body.requirement.calls),
        )
        .chain(evidence.roots.iter().flat_map(|root| &root.selections))
    {
        budget.visit()?;
        if let CallRequirement::Native(import) = call {
            natives
                .entry(&import.instance)
                .or_default()
                .push(import.as_ref());
        }
    }
    let validator = Validator {
        bodies,
        natives,
        catalog,
    };
    for body in &evidence.bodies {
        let scope = Scope::new(body.requirement.generic.as_ref());
        scope.types(
            body.signature
                .params
                .iter()
                .chain([&body.signature.result])
                .chain(&body.captures),
            catalog,
            budget,
        )?;
        for call in &body.requirement.calls {
            validator.call(call, &scope, budget)?;
        }
    }
    for call in &evidence.selections {
        validator.call(call, &Scope::new(None), budget)?;
    }
    for root in &evidence.roots {
        for call in &root.selections {
            validator.call(call, &Scope::new(None), budget)?;
        }
    }
    Ok(())
}

struct Scope<'a> {
    parameters: &'a [GenericParam],
    assumptions: &'a [GenericBound],
}

impl<'a> Scope<'a> {
    fn new(body: Option<&'a GenericBody>) -> Self {
        Self {
            parameters: body.map_or(&[], |body| &body.parameters),
            assumptions: body.map_or(&[], |body| &body.bounds),
        }
    }

    fn types<'b>(
        &self,
        types: impl IntoIterator<Item = &'b Ty>,
        catalog: &ProofCatalog<'_>,
        budget: &mut Budget<'_>,
    ) -> Result<(), UpdateError> {
        for ty in types {
            budget.visit()?;
            if !types_in_scope([ty], self.parameters, budget.cancel, catalog.definitions()) {
                return Err(UpdateError::InvalidEvidence);
            }
        }
        Ok(())
    }
}

struct Validator<'a, 'b> {
    bodies: HashMap<&'a ConcreteFunctionIdentity, &'a BodyRecord>,
    catalog: &'a ProofCatalog<'b>,
    natives: HashMap<&'a ConcreteFunctionIdentity, Vec<&'a NativeImport>>,
}

impl Validator<'_, '_> {
    fn call(
        &self,
        call: &CallRequirement,
        scope: &Scope<'_>,
        budget: &mut Budget<'_>,
    ) -> Result<(), UpdateError> {
        budget.visit()?;
        let cancel = budget.cancel;
        let valid = match call {
            CallRequirement::Direct(instance) => {
                scope.types(&instance.arguments, self.catalog, budget)?;
                true
            }
            CallRequirement::RetainedBody(application) => {
                scope.types(&application.arguments, self.catalog, budget)?;
                let body = self
                    .bodies
                    .get(&application.body)
                    .ok_or_else(|| UpdateError::MissingBody(application.body.clone()))?;
                body.requirement
                    .generic
                    .as_ref()
                    .map_or(0, |generic| generic.parameters.len())
                    == application.arguments.len()
            }
            CallRequirement::Shared(call) => {
                let (generic, signature) = match call.implementation {
                    CallableImplementation::Script => {
                        let body = self
                            .bodies
                            .get(&call.instance)
                            .ok_or_else(|| UpdateError::MissingBody(call.instance.clone()))?;
                        (body.requirement.generic.as_ref(), &body.signature)
                    }
                    CallableImplementation::Native(binding) => {
                        let imports = self
                            .natives
                            .get(&call.instance)
                            .ok_or_else(|| UpdateError::MissingNative(call.instance.clone()))?;
                        // Placement-specific witness environments are not unique by
                        // native application. Check every route; never select one.
                        for import in imports {
                            budget.visit()?;
                            if import.binding != binding
                                || !call.check(
                                    import
                                        .generic
                                        .as_ref()
                                        .ok_or(UpdateError::InvalidEvidence)?,
                                    &import.signature,
                                    self.catalog,
                                    scope.parameters,
                                    scope.assumptions,
                                    cancel,
                                )?
                            {
                                return Err(UpdateError::InvalidEvidence);
                            }
                        }
                        return Ok(());
                    }
                    _ => return Err(UpdateError::InvalidEvidence),
                };
                let generic = generic.ok_or(UpdateError::InvalidEvidence)?;
                call.check(
                    generic,
                    signature,
                    self.catalog,
                    scope.parameters,
                    scope.assumptions,
                    cancel,
                )?
            }
            CallRequirement::Interface(call) => {
                let contract = self
                    .catalog
                    .trait_contract(&call.interface.declaration)
                    .ok_or(UpdateError::InvalidEvidence)?;
                call.check(
                    contract,
                    self.catalog,
                    scope.parameters,
                    scope.assumptions,
                    cancel,
                )?
            }
            CallRequirement::Native(import) => {
                let declaration = self
                    .catalog
                    .native_declaration(import.instance.declaration)
                    .ok_or(UpdateError::InvalidEvidence)?;
                import.matches_declaration(declaration, self.catalog, cancel)?
            }
            CallRequirement::Operation(operation) => {
                operation.valid(self.catalog, scope.parameters, scope.assumptions, cancel)?
            }
            CallRequirement::Construction(construction) => {
                construction.valid(self.catalog, scope.parameters, scope.assumptions, cancel)?
            }
            CallRequirement::Layout(ty) => {
                scope.types([ty], self.catalog, budget)?;
                true
            }
            CallRequirement::Unknown(_) => true,
        };
        if valid {
            Ok(())
        } else {
            Err(UpdateError::InvalidEvidence)
        }
    }
}
