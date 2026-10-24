use crate::instruction::ConstructionTarget;
use crate::{
    artifact::{ArtifactFingerprint, DependencyFingerprint},
    instruction::{BytecodeInstruction, CallTarget},
    module::BytecodeModule,
    program::verified::ProgramVerification,
    suspension::FlowBudget,
    trait_bounds::{self, preparation::VerificationResources},
    verifier,
    verifier::BytecodeVerificationError,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, mapping::DefinitionMappingError, table::DefinitionTable},
};
use kagari_contract::{
    exports::ProgramExports,
    host::external::bindings_match as external_bindings_match,
    layout, types as abi,
    types::{InterfaceBridge, PublicItem, named::correspondence::validate_inventory, verify},
    updates::{UpdateEvidence, limits::UpdateError},
};
use kagari_types::{declaration::TypeDefKind, language::Protocol, ty::Ty};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    slice,
};

mod construction;
pub mod exports;
pub mod identity;
pub mod updates;
pub mod verified;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ModuleRef(u32);

impl ModuleRef {
    pub fn new(index: usize) -> Self {
        Self(index as u32)
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Editable executable dependency records in the caller's definition context.
/// Module and function slots are program-local; module references may form cycles.
/// Verification produces separately owned immutable evidence and a retained table.
#[derive(Debug, Clone)]
pub struct BytecodeProgram {
    pub root: ModuleRef,
    pub modules: Vec<BytecodeModule>,
    pub exports: ProgramExports,
    pub update_evidence: UpdateEvidence,
}

/// Check a complete executable graph against its retained identity prefix.
///
/// This does not allocate names or change the table. Construction/import boundaries
/// collect implicit names first; foreign, absent or later references are rejected.
pub fn verify_program(
    program: &BytecodeProgram,
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<(), BytecodeVerificationError> {
    verify_graph(program, definitions, cancel).map(|_| ())
}

pub(crate) fn verify_graph(
    program: &BytecodeProgram,
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<ProgramVerification, BytecodeVerificationError> {
    let result = verify_graph_inner(program, definitions, cancel);
    cancel
        .check()
        .map_err(|_| BytecodeVerificationError::Cancelled)?;
    result
}

fn verify_graph_inner(
    program: &BytecodeProgram,
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<ProgramVerification, BytecodeVerificationError> {
    cancel
        .check()
        .map_err(|_| BytecodeVerificationError::Cancelled)?;
    identity::validate(
        &program.modules.iter().collect::<Vec<_>>(),
        definitions,
        cancel,
    )?;
    let mut flow_budget = FlowBudget::new(cancel);
    let mut suspensions = Vec::with_capacity(program.modules.len());
    let invalid = || BytecodeVerificationError::InvalidProgramGraph;
    if program.modules.len() > u32::MAX as usize || program.root.index() >= program.modules.len() {
        return Err(invalid());
    }
    for module in &program.modules {
        let mut dependencies = HashSet::new();
        if module.dependencies.iter().any(|dependency| {
            dependency.index() >= program.modules.len() || !dependencies.insert(*dependency)
        }) {
            return Err(invalid());
        }
    }
    let mut identities = HashSet::new();
    for module in &program.modules {
        cancel
            .check()
            .map_err(|_| BytecodeVerificationError::Cancelled)?;
        if !identities.insert(&module.identity) {
            return Err(invalid());
        }
        suspensions.push(verifier::verify_module_with_program(
            module,
            Some(program),
            &mut flow_budget,
            definitions,
        )?);
        #[cfg(test)]
        tests::after_module(cancel);
    }
    validate_inventory(
        program
            .modules
            .iter()
            .flat_map(|module| &module.named_declarations),
        definitions,
        cancel,
    )
    .map_err(|error| match error {
        DefinitionMappingError::Cancelled => BytecodeVerificationError::Cancelled,
        _ => BytecodeVerificationError::InvalidPublicAbi,
    })?;
    let mut resources =
        VerificationResources::new(program.modules.iter().collect(), definitions, cancel)?;
    let mut layouts = HashMap::new();
    let mut enum_layouts = HashMap::new();
    for (index, module) in program.modules.iter().enumerate() {
        cancel
            .check()
            .map_err(|_| BytecodeVerificationError::Cancelled)?;
        for layout in &module.structures {
            if let Some(previous) = layouts.insert((&layout.declaration, &layout.arguments), layout)
                && previous != layout
            {
                return Err(BytecodeVerificationError::InvalidStructLayout);
            }
        }
        for layout in &module.structures {
            let Some(owner) = definitions
                .resolve(layout.declaration)
                .ok()
                .and_then(|view| resources.module_indices.get(view.module()))
                .map(|index| &program.modules[*index])
            else {
                continue;
            };
            let Some(template) = owner.public_items.iter().find(|item| {
                matches!(item, PublicItem::Type(ty) if ty.kind == TypeDefKind::Struct && definitions.resolve(layout.declaration).ok().and_then(|view| view.last()).is_some_and(|part| part.name == ty.name))
            }) else { continue; };
            if !layout::struct_abi_matches(
                slice::from_ref(layout),
                &owner.identity,
                slice::from_ref(template),
                cancel,
                definitions,
            )
            .map_err(|_| BytecodeVerificationError::Cancelled)?
            {
                return Err(BytecodeVerificationError::InvalidStructLayout);
            }
        }
        for layout in &module.enumerations {
            if let Some(previous) =
                enum_layouts.insert((&layout.declaration, &layout.arguments), layout)
                && previous != layout
            {
                return Err(BytecodeVerificationError::InvalidEnumLayout);
            }
        }

        for layout in &module.enumerations {
            let Some(owner) = definitions
                .resolve(layout.declaration)
                .ok()
                .and_then(|view| resources.module_indices.get(view.module()))
                .map(|index| &program.modules[*index])
            else {
                continue;
            };
            let Some(template) = owner.public_items.iter().find(|item| {
                matches!(item, PublicItem::Type(ty) if ty.kind == TypeDefKind::Enum && definitions.resolve(layout.declaration).ok().and_then(|view| view.last()).is_some_and(|part| part.name == ty.name))
            }) else { continue; };
            if !layout::enum_abi_matches(
                slice::from_ref(layout),
                &owner.identity,
                slice::from_ref(template),
                cancel,
                definitions,
            )
            .map_err(|_| BytecodeVerificationError::Cancelled)?
            {
                return Err(BytecodeVerificationError::InvalidEnumLayout);
            }
        }
        let mut reachable = HashSet::new();
        let mut pending = module.dependencies.clone();
        while let Some(dependency) = pending.pop() {
            cancel
                .check()
                .map_err(|_| BytecodeVerificationError::Cancelled)?;
            if reachable.insert(dependency) {
                pending.extend_from_slice(&program.modules[dependency.index()].dependencies);
            }
        }
        let mut indices = reachable
            .iter()
            .map(|module| module.index())
            .collect::<Vec<_>>();
        if !reachable.contains(&ModuleRef::new(index)) {
            indices.push(index);
        }
        indices.sort_unstable();
        let visible_modules = indices
            .iter()
            .map(|index| &program.modules[*index])
            .collect::<Vec<_>>();
        let closure = &visible_modules;
        let owners = resources.receiver_owners(&indices);
        if !external_bindings_match(
            &module.external_types,
            |id| {
                closure.iter().find_map(|owner| {
                    abi::type_contract(&owner.identity, &owner.public_items, &id, definitions)
                })
            },
            cancel,
        )
        .map_err(|_| BytecodeVerificationError::InvalidProgramGraph)?
        {
            return Err(BytecodeVerificationError::InvalidHostInterface(
                "external type dependency disagrees with its defining module".into(),
            ));
        }
        if !verify::trait_references_match(
            &module.trait_references,
            |id| {
                closure.iter().find_map(|owner| {
                    abi::trait_contract(
                        &owner.identity,
                        &owner.public_items,
                        &owner.trait_contracts,
                        &id,
                        definitions,
                    )
                })
            },
            cancel,
        )
        .map_err(|_| BytecodeVerificationError::InvalidInterfaceTable)?
        {
            return Err(BytecodeVerificationError::InvalidInterfaceTable);
        }
        for item in &module.public_items {
            let PublicItem::InterfaceTable(table) = item else {
                continue;
            };
            let Ty::Trait(instance) = &table.trait_type else {
                return Err(BytecodeVerificationError::InvalidInterfaceTable);
            };
            let trait_id = definitions
                .resolve(instance.declaration)
                .map_err(|_| BytecodeVerificationError::InvalidInterfaceTable)?;
            let implementation = definitions
                .resolve(table.declaration)
                .map_err(|_| BytecodeVerificationError::InvalidInterfaceTable)?;
            if let Some(kind) = Protocol::from_definition(&instance.declaration, definitions)
                && table.bridge != Some(InterfaceBridge::Intrinsic)
                && (!kind.host_implementable() && matches!(table.for_type, Ty::Host(_))
                    || trait_id.module() != implementation.module()
                        && owners.owner(&table.for_type, definitions).as_ref()
                            != Some(implementation.module())
                        && kind != Protocol::From
                        && !matches!(
                            table.for_type,
                            Ty::NativeObject(_) | Ty::Struct(_) | Ty::Enum(_) | Ty::Host(_)
                        ))
            {
                return Err(BytecodeVerificationError::InvalidInterfaceTable);
            }
            if trait_id.module() == &module.identity {
                continue;
            }
            if trait_id.segments().count() != 1
                || !trait_id
                    .last()
                    .is_some_and(|part| part.kind == DefinitionKind::Trait && part.occurrence == 0)
            {
                return Err(BytecodeVerificationError::InvalidInterfaceTable);
            }
            let Some((owner_index, owner)) = resources
                .module_indices
                .get(trait_id.module())
                .map(|index| (*index, &program.modules[*index]))
            else {
                return Err(BytecodeVerificationError::InvalidInterfaceTable);
            };
            if !reachable.contains(&ModuleRef::new(owner_index)) {
                return Err(BytecodeVerificationError::InvalidInterfaceTable);
            }
            let Some(trait_name) = trait_id.last().map(|part| part.name) else {
                return Err(BytecodeVerificationError::InvalidInterfaceTable);
            };
            let interface = owner
                .public_items
                .iter()
                .find_map(|item| match item {
                    PublicItem::Trait(interface) if interface.name == trait_name => Some(interface),
                    _ => None,
                })
                .or_else(|| {
                    module
                        .trait_references
                        .iter()
                        .find(|reference| reference.declaration == instance.declaration)
                        .and_then(|reference| {
                            owner.trait_contracts.iter().find(|actual| {
                                actual.declaration == reference.declaration
                                    && actual.abi == reference.abi
                            })
                        })
                        .map(|actual| &actual.abi)
                });
            let Some(interface) = interface else {
                return Err(BytecodeVerificationError::InvalidInterfaceTable);
            };
            if !verify::interface_contract_matches(table, interface, cancel, definitions) {
                return Err(BytecodeVerificationError::InvalidInterfaceTable);
            }
        }
        let scope = resources.scope(&indices, cancel)?;
        trait_bounds::verify_trait_bounds(index, Some(program), &resources, &scope, cancel)?;
        for instruction in module
            .functions
            .iter()
            .flat_map(|function| &function.instructions)
        {
            let target = match instruction {
                BytecodeInstruction::Call {
                    callee: CallTarget::ModuleFunction { module: target, .. },
                    ..
                }
                | BytecodeInstruction::Call {
                    callee: CallTarget::Shared { module: target, .. },
                    ..
                }
                | BytecodeInstruction::Call {
                    callee: CallTarget::InterfaceMethod { module: target, .. },
                    ..
                }
                | BytecodeInstruction::MakeInterface {
                    target: ConstructionTarget::Declared { module: target, .. },
                    ..
                } => Some(target),
                _ => None,
            };
            if let Some(target) = target
                && *target != ModuleRef::new(index)
                && !reachable.contains(target)
            {
                return Err(invalid());
            }
        }
        if module.functions.iter().any(|function| {
            function.metadata.debug.source_module.is_some_and(|origin| {
                origin != ModuleRef::new(index) && !reachable.contains(&origin)
            })
        }) {
            return Err(invalid());
        }
    }
    let mut reachable = HashSet::new();
    let mut pending = vec![program.root];
    while let Some(module) = pending.pop() {
        cancel
            .check()
            .map_err(|_| BytecodeVerificationError::Cancelled)?;
        if reachable.insert(module) {
            pending.extend_from_slice(&program.modules[module.index()].dependencies);
        }
    }
    if reachable.len() != program.modules.len() {
        return Err(invalid());
    }
    let update_coverage = construction::verify(program, definitions, cancel)?;
    let scope = resources.scope(&(0..program.modules.len()).collect::<Vec<_>>(), cancel)?;
    let exports = exports::verify(program, definitions, &scope.catalog, cancel)?;
    updates::verify(
        &program.modules.iter().collect::<Vec<_>>(),
        &program.update_evidence,
        definitions,
        &scope.catalog,
        cancel,
    )
    .map_err(|cause| {
        if cause == UpdateError::Cancelled {
            BytecodeVerificationError::Cancelled
        } else {
            BytecodeVerificationError::UpdateEvidence(cause)
        }
    })?;
    Ok(ProgramVerification {
        suspensions,
        exports,
        update_coverage,
    })
}

impl BytecodeProgram {
    pub fn dependency_fingerprints(
        &self,
        definitions: &DefinitionTable,
    ) -> Result<Vec<DependencyFingerprint>, DefinitionMappingError> {
        self.modules
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != self.root.index())
            .map(|(_, module)| {
                Ok(DependencyFingerprint {
                    module_id: module.identity.clone(),
                    fingerprint: ArtifactFingerprint::of_module(module, definitions)?,
                })
            })
            .collect()
    }
}

mod mapping;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod type_exports;
