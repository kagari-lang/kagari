//! Declaration authority is independent of the supplied executable certificate.
use crate::module::BytecodeModule;
use crate::program::updates::context::Context;
use kagari_common::cancellation::CancellationToken;
use kagari_common::identity::{
    mapping::DefinitionMappingError,
    table::{DefinitionId, DefinitionTable},
};
use kagari_contract::{
    ids::FunctionRef,
    types::{ConcreteFunctionIdentity, named::NamedContract},
    updates::{
        BodyClassification, BodyRecord, CallRequirement, CurrentTarget, StaticInitializer,
        admission::Inventory,
        limits::{Budget, UpdateError},
    },
};
use std::collections::{HashMap, HashSet};

pub(super) struct Index {
    pub sources: Vec<DefinitionId>,
    pub bodies: Vec<ConcreteFunctionIdentity>,
    pub targets: Vec<CurrentTarget>,
    pub initializers: Vec<StaticInitializer>,
    pub selections: Vec<CallRequirement>,
}

impl Index {
    pub fn borrowed(&self) -> Inventory<'_> {
        Inventory {
            sources: &self.sources,
            bodies: &self.bodies,
            targets: &self.targets,
            initializers: &self.initializers,
            selections: &self.selections,
        }
    }
}

pub(super) fn collect(
    modules: &[&BytecodeModule],
    bodies: &[BodyRecord],
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
    budget: &mut Budget<'_>,
) -> Result<Index, UpdateError> {
    let mut index = Index {
        sources: vec![],
        bodies: vec![],
        targets: vec![],
        initializers: vec![],
        selections: vec![],
    };
    let extraction = Context::new(modules, definitions, cancel, budget)?;
    let mut body_index = HashMap::new();
    for body in bodies {
        budget.visit()?;
        if body_index.insert(&body.requirement.origin, body).is_some() {
            return Err(UpdateError::InvalidEvidence);
        }
    }
    let mut sources = HashSet::new();
    for (module_index, module) in modules.iter().enumerate() {
        for declaration in &module.named_declarations {
            budget.visit()?;
            if !matches!(declaration.contract, NamedContract::Alias(_))
                && sources.insert(declaration.declaration)
            {
                index.sources.push(declaration.declaration);
            }
        }
        for slot in &module.module_slots {
            budget.visit()?;
            if slot.contract.supplier != module.identity {
                continue;
            }
            if sources.insert(slot.contract.declaration) {
                index.sources.push(slot.contract.declaration);
            }
            index.initializers.push(StaticInitializer {
                declaration: slot.contract.declaration,
            });
        }
        for function in &module.functions {
            let identity = function
                .identity
                .as_ref()
                .ok_or(UpdateError::InvalidEvidence)?;
            budget.visit()?;
            index.bodies.push(identity.clone());
            let authored = extraction.authored(identity.declaration);
            if extraction.named.contains(&authored) {
                let body = body_index
                    .get(&identity)
                    .ok_or(UpdateError::InvalidEvidence)?;
                if body.classification != BodyClassification::Named(authored) {
                    return Err(UpdateError::InvalidEvidence);
                }
                index.targets.push(CurrentTarget {
                    application: ConcreteFunctionIdentity {
                        declaration: authored,
                        arguments: identity.arguments.clone(),
                    },
                    body: identity.clone(),
                    module: u32::try_from(module_index).map_err(|_| UpdateError::LimitExceeded)?,
                    function: FunctionRef::new(function.id.index()),
                });
            }
        }
    }
    // Source collectors use authored declaration order; executable module placement
    // does not define the order of static initializer identities.
    index.initializers.sort_by_key(|initializer| {
        definitions
            .resolve(initializer.declaration)
            .map(|view| view.to_path())
            .ok()
    });
    for id in &index.sources {
        definitions
            .resolve(*id)
            .map_err(DefinitionMappingError::from)?;
    }
    Ok(index)
}
