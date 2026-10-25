//! Invocation-local indexes over immutable executable/declaration records.
use crate::instruction::BytecodeInstruction;
use crate::module::BytecodeModule;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_contract::{
    types::{
        ConcreteFunctionIdentity, InterfaceBridge, PublicItem, named::NamedContract, trait_contract,
    },
    updates::limits::{Budget, UpdateError},
};
use kagari_types::{callable::CallableImplementation, ty::Ty};
use std::collections::{HashMap, HashSet};

pub(super) struct Creation {
    pub parent: ConcreteFunctionIdentity,
    pub future: bool,
    pub captures: Vec<Ty>,
}

pub(super) struct Context<'a> {
    pub modules: &'a [&'a BytecodeModule],
    pub definitions: &'a DefinitionTable,
    pub cancel: &'a CancellationToken,
    pub named: HashSet<DefinitionId>,
    pub bodies: HashSet<ConcreteFunctionIdentity>,
    pub bridges: HashSet<ConcreteFunctionIdentity>,
    origins: HashMap<DefinitionId, DefinitionId>,
    default_sources: HashMap<DefinitionId, DefinitionId>,
    pub creations: HashMap<ConcreteFunctionIdentity, Vec<Creation>>,
}

impl<'a> Context<'a> {
    pub fn new(
        modules: &'a [&'a BytecodeModule],
        definitions: &'a DefinitionTable,
        cancel: &'a CancellationToken,
        budget: &mut Budget<'_>,
    ) -> Result<Self, UpdateError> {
        let mut value = Self {
            modules,
            definitions,
            cancel,
            named: HashSet::new(),
            bodies: HashSet::new(),
            bridges: HashSet::new(),
            origins: HashMap::new(),
            default_sources: HashMap::new(),
            creations: HashMap::new(),
        };
        for module in modules {
            budget.visit()?;
            for record in &module.named_declarations {
                budget.visit()?;
                if matches!(record.contract, NamedContract::Function(_)) {
                    value.named.insert(record.declaration);
                }
            }
            for origin in &module.function_origins {
                budget.visit()?;
                if value
                    .origins
                    .insert(origin.placement, origin.authored)
                    .is_some()
                {
                    return Err(UpdateError::InvalidEvidence);
                }
            }
            for function in &module.functions {
                budget.visit()?;
                if !value.bodies.insert(
                    function
                        .identity
                        .clone()
                        .ok_or(UpdateError::InvalidEvidence)?,
                ) {
                    return Err(UpdateError::InvalidEvidence);
                }
            }
        }
        value.index_table_methods(budget)?;
        for module in modules {
            for owner in &module.functions {
                for instruction in &owner.instructions {
                    budget.visit()?;
                    let (target, values, future) = match instruction {
                        BytecodeInstruction::MakeClosure {
                            function, captures, ..
                        } => (function, captures, false),
                        BytecodeInstruction::MakeFuture {
                            function,
                            arguments,
                            ..
                        } => (function, arguments, true),
                        _ => continue,
                    };
                    let target = module
                        .functions
                        .get(target.index())
                        .ok_or(UpdateError::InvalidEvidence)?;
                    budget.records(values.len() + 1)?;
                    let captures = values
                        .iter()
                        .map(|value| {
                            budget.visit()?;
                            let ty = owner
                                .metadata
                                .semantic
                                .slots
                                .get(&value.index())
                                .ok_or(UpdateError::InvalidEvidence)?;
                            if !ty.within_wire_limits() {
                                return Err(UpdateError::LimitExceeded);
                            }
                            Ok(ty.clone())
                        })
                        .collect::<Result<_, UpdateError>>()?;
                    value
                        .creations
                        .entry(
                            target
                                .identity
                                .clone()
                                .ok_or(UpdateError::InvalidEvidence)?,
                        )
                        .or_default()
                        .push(Creation {
                            parent: owner.identity.clone().ok_or(UpdateError::InvalidEvidence)?,
                            future,
                            captures,
                        });
                }
            }
        }
        Ok(value)
    }

    fn index_table_methods(&mut self, budget: &mut Budget<'_>) -> Result<(), UpdateError> {
        for module in self.modules {
            let mut local_bodies = HashSet::new();
            for function in &module.functions {
                budget.visit()?;
                local_bodies.insert(
                    function
                        .identity
                        .as_ref()
                        .ok_or(UpdateError::InvalidEvidence)?,
                );
            }
            for item in &module.public_items {
                budget.visit()?;
                let PublicItem::InterfaceTable(table) = item else {
                    continue;
                };

                let Ty::Trait(interface) = &table.trait_type else {
                    return Err(UpdateError::InvalidEvidence);
                };
                for method in &table.methods {
                    budget.visit()?;
                    if method.implementation != CallableImplementation::Script {
                        continue;
                    }
                    let declaration = self
                        .definitions
                        .lookup_child(table.declaration, DefinitionKind::Method, &method.name, 0)
                        .ok_or(UpdateError::InvalidEvidence)?;
                    if table.bridge == Some(InterfaceBridge::Intrinsic) {
                        // Bridge methods are generated bodies, not authored declarations.
                        // Full table/signature/forwarding validation remains with the verifier.
                        let instance = ConcreteFunctionIdentity {
                            declaration,
                            arguments: Vec::new(),
                        };
                        if !local_bodies.contains(&instance) {
                            return Err(UpdateError::InvalidEvidence);
                        }
                        self.bridges.insert(instance);
                        continue;
                    }
                    if self.named.contains(&declaration) {
                        continue;
                    }
                    let mut contract = None;
                    for owner in self.modules {
                        budget.visit()?;
                        if let Some(found) = trait_contract(
                            &owner.identity,
                            &owner.public_items,
                            &owner.trait_contracts,
                            &interface.declaration,
                            self.definitions,
                        ) {
                            contract = Some(found);
                            break;
                        }
                    }
                    let contract = contract.ok_or(UpdateError::InvalidEvidence)?;
                    let mut has_default = false;
                    for required in &contract.methods {
                        budget.visit()?;
                        if required.name == method.name {
                            has_default = required.implementation == CallableImplementation::Script;
                            break;
                        }
                    }
                    let source = self
                        .definitions
                        .lookup_child(
                            interface.declaration,
                            DefinitionKind::Method,
                            &method.name,
                            0,
                        )
                        .ok_or(UpdateError::InvalidEvidence)?;
                    if !has_default || !self.named.contains(&source) {
                        return Err(UpdateError::InvalidEvidence);
                    }
                    // The impl owns current-call selection; the trait owns the source.
                    // This is not an authored function or an imported-local origin.
                    self.named.insert(declaration);
                    self.default_sources.insert(declaration, source);
                }
            }
        }
        Ok(())
    }

    pub fn source(&self, declaration: DefinitionId) -> DefinitionId {
        self.default_sources
            .get(&declaration)
            .copied()
            .unwrap_or(declaration)
    }

    pub fn authored(&self, declaration: DefinitionId) -> DefinitionId {
        self.origins
            .get(&declaration)
            .copied()
            .unwrap_or(declaration)
    }
}
