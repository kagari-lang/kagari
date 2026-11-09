//! Product-owned callable facts, independent of Runtime selection or authority.
use crate::{
    error::RuntimeError,
    module::{VerifiedProgram, dispatch::invalid},
};
use kagari_bytecode::{instruction::NativeImportId, module::CallableTarget, program::ModuleRef};
use kagari_common::identity::{
    ModuleIdentity,
    table::{DefinitionId, DefinitionTable},
};
use kagari_contract::{
    callable::generic::GenericBody, native_import::NativeImport, types::ConcreteFunctionIdentity,
    updates::BodyClassification,
};
use kagari_types::callable::Signature;
use std::{collections::HashMap, sync::Arc};

pub(super) type Placement = (ModuleRef, CallableTarget);

#[derive(Debug)]
pub(super) enum RouteSelection {
    Named {
        application: ConcreteFunctionIdentity,
        shared: bool,
    },
    Native {
        provider: ModuleIdentity,
        import: Arc<NativeImport>,
    },
    BodyOwned,
}

#[derive(Debug)]
pub(super) struct TargetRecord {
    pub(super) selection: Arc<RouteSelection>,
    pub(super) signature: Signature,
    pub(super) generic: Option<GenericBody>,
    pub(super) classification: Option<BodyClassification>,
    pub(super) binders: Vec<DefinitionId>,
}

/// This cache holds types and callable identities, never code owners, registry
/// capabilities or selected Runtime entries. Its exact owner is one verified
/// product; source attachment and installation do not change these facts.
#[derive(Debug)]
pub(in crate::module) struct DispatchFacts {
    targets: HashMap<Placement, TargetRecord>,
}

impl DispatchFacts {
    pub(in crate::module) fn prepare(program: &VerifiedProgram) -> Result<Self, RuntimeError> {
        let bodies: HashMap<_, _> = program
            .update_evidence()
            .bodies
            .iter()
            .map(|body| (&body.requirement.origin, body))
            .collect();
        let mut named = HashMap::new();
        for target in &program.update_evidence().targets {
            let placement = (
                ModuleRef::new(target.module as usize),
                CallableTarget::Script(target.function),
            );
            if named.insert(placement, target).is_some() {
                return Err(invalid("duplicate checked current placement"));
            }
        }
        let mut targets = HashMap::new();
        for (module_index, module) in program.modules().iter().enumerate() {
            let slot = ModuleRef::new(module_index);
            for function in &module.functions {
                let identity = function
                    .identity
                    .as_ref()
                    .ok_or_else(|| invalid("missing checked body identity"))?;
                let body = bodies
                    .get(identity)
                    .ok_or_else(|| invalid("missing checked body classification"))?;
                let placement = (slot, CallableTarget::Script(function.id));
                let selection = if let Some(target) = named.get(&placement) {
                    if target.body != *identity
                        || !matches!(body.classification, BodyClassification::Named(_))
                    {
                        return Err(invalid("current target body correspondence"));
                    }
                    RouteSelection::Named {
                        application: target.application.clone(),
                        shared: body.requirement.generic.is_some(),
                    }
                } else {
                    if matches!(body.classification, BodyClassification::Named(_)) {
                        return Err(invalid("named body has no current target"));
                    }
                    RouteSelection::BodyOwned
                };
                let declaration = match &selection {
                    RouteSelection::Named { application, .. } => application.declaration,
                    _ => identity.declaration,
                };
                targets.insert(
                    placement,
                    TargetRecord {
                        selection: Arc::new(selection),
                        signature: body.signature.clone(),
                        generic: body.requirement.generic.clone(),
                        classification: Some(body.classification.clone()),
                        binders: target_owners(
                            body.requirement.generic.as_ref(),
                            declaration,
                            program.definitions(),
                        )?,
                    },
                );
            }
            for (index, import) in module.native_imports.iter().enumerate() {
                targets.insert(
                    (slot, CallableTarget::Native(NativeImportId::new(index))),
                    TargetRecord {
                        selection: Arc::new(RouteSelection::Native {
                            provider: program
                                .definitions()
                                .resolve(import.binding)
                                .map_err(|error| invalid(error.to_string()))?
                                .module()
                                .clone(),
                            import: Arc::new(import.clone()),
                        }),
                        signature: import.signature.clone(),
                        generic: import.generic.clone(),
                        classification: None,
                        binders: target_owners(
                            import.generic.as_ref(),
                            import.instance.declaration,
                            program.definitions(),
                        )?,
                    },
                );
            }
        }
        Ok(Self { targets })
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = (&Placement, &TargetRecord)> {
        self.targets.iter()
    }

    pub(super) fn target(
        &self,
        module: ModuleRef,
        target: CallableTarget,
    ) -> Option<&TargetRecord> {
        self.targets.get(&(module, target))
    }
}

fn target_owners(
    body: Option<&GenericBody>,
    declaration: DefinitionId,
    definitions: &DefinitionTable,
) -> Result<Vec<DefinitionId>, RuntimeError> {
    let mut owners = Vec::new();
    // The selected member fixes its original impl owner even for nongeneric
    // methods in semantically equivalent inherent blocks.
    let mut next = Some(declaration);
    while let Some(id) = next {
        owners.push(id);
        next = definitions
            .parent(id)
            .map_err(|error| invalid(error.to_string()))?;
    }
    if let Some(body) = body {
        for parameter in &body.parameters {
            if !owners.contains(&parameter.owner) {
                owners.push(parameter.owner);
            }
        }
    }
    Ok(owners)
}
