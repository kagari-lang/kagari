use crate::{
    artifact::{ArtifactFingerprint, DependencyFingerprint},
    instruction::{BytecodeInstruction, CallTarget},
    module::BytecodeModule,
    suspension::{FlowBudget, ProgramSuspensions},
    trait_bounds, verifier,
    verifier::BytecodeVerificationError,
};
use kagari_common::identity::{
    DefinitionKind, mapping::DefinitionMappingError, table::DefinitionTable,
};
use kagari_contract::{
    host, layout,
    types::{PublicItem, verify},
};
use kagari_types::{
    declaration::{TypeDefKind, ownership::ReceiverOwners},
    language::Protocol,
    ty::Ty,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    slice,
};

mod construction;
pub mod identity;
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
}

/// Check a complete executable graph against its retained identity prefix.
///
/// This does not allocate names or change the table. Construction/import boundaries
/// collect implicit names first; foreign, absent or later references are rejected.
pub fn verify_program(
    program: &BytecodeProgram,
    definitions: &DefinitionTable,
) -> Result<(), BytecodeVerificationError> {
    verify_with_suspensions(program, definitions).map(|_| ())
}

pub(crate) fn verify_with_suspensions(
    program: &BytecodeProgram,
    definitions: &DefinitionTable,
) -> Result<ProgramSuspensions, BytecodeVerificationError> {
    identity::validate(&program.modules.iter().collect::<Vec<_>>(), definitions)?;
    let mut flow_budget = FlowBudget::default();
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
    let mut layouts = HashMap::new();
    let mut enum_layouts = HashMap::new();
    let mut hosts = HashMap::new();
    let mut symbols = HashMap::new();
    let mut host_types = HashMap::new();
    let mut host_type_symbols = HashMap::new();
    for (index, module) in program.modules.iter().enumerate() {
        if !identities.insert(&module.identity) {
            return Err(invalid());
        }
        for layout in &module.structures {
            if let Some(previous) = layouts.insert((&layout.declaration, &layout.arguments), layout)
                && previous != layout
            {
                return Err(BytecodeVerificationError::InvalidStructLayout);
            }
        }
        for layout in &module.structures {
            let Some(owner) = program.modules.iter().find(|owner| {
                definitions
                    .resolve(layout.declaration)
                    .is_ok_and(|view| view.module() == &owner.identity)
            }) else {
                continue;
            };
            let Some(template) = owner.public_items.iter().find(|item| {
                matches!(item, PublicItem::Type(ty) if ty.kind == TypeDefKind::Struct && definitions.resolve(layout.declaration).ok().and_then(|view| view.last()).is_some_and(|part| part.name == ty.name))
            }) else { continue; };
            if !layout::struct_abi_matches(
                slice::from_ref(layout),
                &owner.identity,
                slice::from_ref(template),
                &Default::default(),
                definitions,
            )
            .expect("bytecode verification uses an uncancelled token")
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
        for declaration in &module.host_interface.functions {
            if hosts
                .insert(&declaration.id, declaration)
                .is_some_and(|previous| !previous.matches_binding(declaration))
                || symbols
                    .insert(&declaration.symbol, &declaration.id)
                    .is_some_and(|previous| previous != &declaration.id)
            {
                return Err(BytecodeVerificationError::InvalidHostInterface(
                    "conflicting declarations across program members".into(),
                ));
            }
        }
        for declaration in &module.host_interface.types {
            if host_types
                .insert(&declaration.id, declaration)
                .is_some_and(|previous| !previous.matches_binding(declaration))
                || host_type_symbols
                    .insert(&declaration.symbol, &declaration.id)
                    .is_some_and(|previous| previous != &declaration.id)
            {
                return Err(BytecodeVerificationError::InvalidHostInterface(
                    "conflicting host type declarations across program members".into(),
                ));
            }
        }
        suspensions.push(verifier::verify_module_with_program(
            module,
            Some(program),
            &mut flow_budget,
            definitions,
        )?);
        for owner in &program.modules {
            if owner.identity != module.identity
                && !host::trait_bindings_match(
                    &module.host_interface,
                    &owner.identity,
                    &owner.public_items,
                    &owner.trait_contracts,
                    &Default::default(),
                    definitions,
                )
                .expect("bytecode verification uses an uncancelled token")
            {
                return Err(BytecodeVerificationError::InvalidHostInterface(
                    "host trait table disagrees with dependency trait ABI".into(),
                ));
            }
        }
        for layout in &module.enumerations {
            let Some(owner) = program.modules.iter().find(|owner| {
                definitions
                    .resolve(layout.declaration)
                    .is_ok_and(|view| view.module() == &owner.identity)
            }) else {
                continue;
            };
            let Some(template) = owner.public_items.iter().find(|item| {
                matches!(item, PublicItem::Type(ty) if ty.kind == TypeDefKind::Enum && definitions.resolve(layout.declaration).ok().and_then(|view| view.last()).is_some_and(|part| part.name == ty.name))
            }) else { continue; };
            if !layout::enum_abi_matches(
                slice::from_ref(layout),
                &owner.identity,
                slice::from_ref(template),
                &Default::default(),
                definitions,
            )
            .expect("bytecode verification uses an uncancelled token")
            {
                return Err(BytecodeVerificationError::InvalidEnumLayout);
            }
        }
        let mut reachable = HashSet::new();
        let mut pending = module.dependencies.clone();
        while let Some(dependency) = pending.pop() {
            if reachable.insert(dependency) {
                pending.extend_from_slice(&program.modules[dependency.index()].dependencies);
            }
        }
        let mut closure = reachable
            .iter()
            .map(|dependency| &program.modules[dependency.index()])
            .collect::<Vec<_>>();
        if !reachable.contains(&ModuleRef::new(index)) {
            closure.push(module);
        }
        let owners = ReceiverOwners::from_types(closure.iter().flat_map(|module| {
            module.public_items.iter().filter_map(|item| {
                if let PublicItem::Type(ty) = item {
                    Some((&module.identity, ty))
                } else {
                    None
                }
            })
        }))
        .map_err(|_| BytecodeVerificationError::InvalidInterfaceTable)?;
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
            let Some((owner_index, owner)) = program
                .modules
                .iter()
                .enumerate()
                .find(|(_, owner)| &owner.identity == trait_id.module())
            else {
                return Err(BytecodeVerificationError::InvalidInterfaceTable);
            };
            if !reachable.contains(&ModuleRef::new(owner_index)) {
                return Err(BytecodeVerificationError::InvalidInterfaceTable);
            }
            let Some(trait_name) = trait_id.last().map(|part| part.name) else {
                return Err(BytecodeVerificationError::InvalidInterfaceTable);
            };
            let Some(PublicItem::Trait(interface)) = owner.public_items.iter().find(
                |item| matches!(item, PublicItem::Trait(interface) if interface.name == trait_name),
            ) else {
                return Err(BytecodeVerificationError::InvalidInterfaceTable);
            };
            if !verify::interface_contract_matches(
                table,
                interface,
                &Default::default(),
                definitions,
            ) {
                return Err(BytecodeVerificationError::InvalidInterfaceTable);
            }
        }
        trait_bounds::verify_trait_bounds(module, &closure, Some(program), definitions)?;
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
                | BytecodeInstruction::MakeInterface { module: target, .. } => Some(target),
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
        if reachable.insert(module) {
            pending.extend_from_slice(&program.modules[module.index()].dependencies);
        }
    }
    if reachable.len() != program.modules.len() {
        return Err(invalid());
    }
    construction::verify(program, definitions)?;
    Ok(suspensions)
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
