//! Checked table projection. Coverage and semantic solving remain UP01's owners.
use crate::{
    error::RuntimeError,
    module::{
        Installation, VerifiedProgram,
        dispatch::{
            PublicationDispatch, PublicationToken,
            identity::{DeclarationIndex, PublicationDefinitions},
            invalid,
        },
    },
};
use kagari_bytecode::{instruction::NativeImportId, module::CallableTarget, program::ModuleRef};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        ModuleIdentity,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_contract::{
    callable::generic::GenericBody, native_import::NativeImport, types::ConcreteFunctionIdentity,
    updates::BodyClassification,
};
use kagari_types::callable::Signature;
use std::{collections::HashMap, sync::Arc};

type Placement = (ModuleRef, CallableTarget);

#[derive(Debug, Clone)]
pub(super) enum RouteSelection {
    Named {
        application: ConcreteFunctionIdentity,
        shared: bool,
    },
    Native {
        module: ModuleIdentity,
        import: Arc<NativeImport>,
    },
    BodyOwned,
}

#[derive(Debug)]
pub(super) struct TargetRecord {
    pub(super) selection: RouteSelection,
    pub(super) signature: Signature,
    pub(super) generic: Option<GenericBody>,
    pub(super) classification: Option<BodyClassification>,
    pub(super) binders: Vec<DefinitionId>,
}

#[derive(Debug)]
struct NativeTarget {
    module: ModuleIdentity,
    contract: NativeImport,
    placement: Placement,
}

#[derive(Debug, Default)]
pub(super) struct DispatchTables {
    targets: HashMap<Placement, TargetRecord>,
    named: HashMap<ConcreteFunctionIdentity, Placement>,
    natives: HashMap<ConcreteFunctionIdentity, Vec<NativeTarget>>,
}

impl PublicationDispatch {
    pub(in crate::module) fn prepare(
        installation: &Installation,
        token: PublicationToken,
        previous: Option<&Self>,
        program: &VerifiedProgram,
    ) -> Result<Self, RuntimeError> {
        let (index, scope) = DeclarationIndex::prepare(
            installation,
            token,
            previous.map(|old| &old.index),
            program,
        )?;
        let tables = DispatchTables::prepare(program, &scope)?;
        Ok(Self {
            index,
            scope,
            tables,
        })
    }
}

impl DispatchTables {
    fn prepare(
        program: &VerifiedProgram,
        scope: &PublicationDefinitions,
    ) -> Result<Self, RuntimeError> {
        let mut result = Self::default();
        let cancel = CancellationToken::default();
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
            let application = scope.canonicalize(scope, &target.application, &cancel)?;
            if result.named.insert(application, placement).is_some() {
                return Err(invalid("duplicate current callable application"));
            }
        }
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
                result.targets.insert(
                    placement,
                    TargetRecord {
                        selection,
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
                let placement = (slot, CallableTarget::Native(NativeImportId::new(index)));
                let canonical = scope.canonicalize(scope, import, &cancel)?;
                result
                    .natives
                    .entry(canonical.instance.clone())
                    .or_default()
                    .push(NativeTarget {
                        module: module.identity.clone(),
                        contract: canonical,
                        placement,
                    });
                result.targets.insert(
                    placement,
                    TargetRecord {
                        selection: RouteSelection::Native {
                            module: module.identity.clone(),
                            import: Arc::new(import.clone()),
                        },
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
        Ok(result)
    }

    pub(super) fn target(
        &self,
        module: ModuleRef,
        target: CallableTarget,
    ) -> Option<&TargetRecord> {
        self.targets.get(&(module, target))
    }

    pub(super) fn select(
        &self,
        current: &PublicationDefinitions,
        source: &PublicationDefinitions,
        selection: &RouteSelection,
        cancel: &CancellationToken,
    ) -> Result<Placement, RuntimeError> {
        match selection {
            RouteSelection::Named {
                application,
                shared,
            } => {
                let application = current.canonicalize(source, application, cancel)?;
                let placement = self
                    .named
                    .get(&application)
                    .copied()
                    .ok_or_else(|| invalid("missing admitted current application"))?;
                if self.targets[&placement].generic.is_some() != *shared {
                    return Err(invalid("current shared entry contract changed"));
                }
                Ok(placement)
            }
            RouteSelection::Native { module, import } => {
                let contract = current.canonicalize(source, import.as_ref(), cancel)?;
                self.natives
                    .get(&contract.instance)
                    .into_iter()
                    .flatten()
                    .find(|target| target.module == *module && target.contract == contract)
                    .map(|target| target.placement)
                    .ok_or_else(|| invalid("missing exact current native import evidence"))
            }
            RouteSelection::BodyOwned => Err(invalid("body-owned target requires exact selection")),
        }
    }
}

fn target_owners(
    body: Option<&GenericBody>,
    declaration: DefinitionId,
    definitions: &DefinitionTable,
) -> Result<Vec<DefinitionId>, RuntimeError> {
    let mut owners = Vec::new();
    // Equivalent inherent blocks share a semantic identity. The selected member
    // fixes its original impl owner even when neither the impl nor method is generic.
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
