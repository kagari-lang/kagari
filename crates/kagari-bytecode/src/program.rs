use crate::ArtifactFingerprint;
use crate::BytecodeInstruction;
use crate::BytecodeModule;
use crate::BytecodeVerificationError;
use crate::CallTarget;
use crate::DependencyFingerprint;
use crate::trait_bounds;
use crate::verifier;
use kagari_abi::host;
use kagari_abi::layout;
use kagari_abi::types as abi;
use kagari_abi::types::AbiType;
use kagari_abi::types::PublicAbiItem;
use kagari_abi::types::verify;
use kagari_common::identity::DefinitionKind;
use kagari_hir::builtin::traits::StandardTrait;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::slice;

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

/// One immutable executable dependency closure. Module and function slots are
/// scoped to this program; module references may form cycles.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BytecodeProgram {
    pub root: ModuleRef,
    #[serde(deserialize_with = "kagari_abi::decode_limits::modules")]
    pub modules: Vec<BytecodeModule>,
}

pub fn verify_program(program: &BytecodeProgram) -> Result<(), BytecodeVerificationError> {
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
        if module.identity.package.0 == "kagari-std" || !identities.insert(&module.identity) {
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
            let Some(owner) = program
                .modules
                .iter()
                .find(|owner| owner.identity == layout.declaration.module)
            else {
                continue;
            };
            let Some(template) = owner.public_items.iter().find(|item| {
                matches!(item, crate::module::PublicAbiItem::Type(ty) if ty.kind == crate::module::TypeAbiKind::Struct && layout.declaration.path.last().is_some_and(|part| part.name == ty.name))
            }) else { continue; };
            if !layout::struct_abi_matches(
                slice::from_ref(layout),
                &owner.identity,
                slice::from_ref(template),
                &Default::default(),
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
        verifier::verify_module_with_program(module, Some(program))?;
        for owner in &program.modules {
            if owner.identity != module.identity
                && !host::trait_bindings_match(
                    &module.host_interface,
                    &owner.identity,
                    &owner.public_items,
                    &owner.trait_contracts,
                    &Default::default(),
                )
                .expect("bytecode verification uses an uncancelled token")
            {
                return Err(BytecodeVerificationError::InvalidHostInterface(
                    "host trait table disagrees with dependency trait ABI".into(),
                ));
            }
        }
        for layout in &module.enumerations {
            let Some(owner) = program
                .modules
                .iter()
                .find(|owner| owner.identity == layout.declaration.module)
            else {
                continue;
            };
            let Some(template) = owner.public_items.iter().find(|item| {
                matches!(item, crate::module::PublicAbiItem::Type(ty) if ty.kind == crate::module::TypeAbiKind::Enum && layout.declaration.path.last().is_some_and(|part| part.name == ty.name))
            }) else { continue; };
            if !layout::enum_abi_matches(
                slice::from_ref(layout),
                &owner.identity,
                slice::from_ref(template),
                &Default::default(),
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
        for item in &module.public_items {
            let PublicAbiItem::InterfaceTable(table) = item else {
                continue;
            };
            let AbiType::Trait(instance) = &table.trait_type else {
                return Err(BytecodeVerificationError::InvalidInterfaceTable);
            };
            if let Some(contract) = abi::standard_trait_contract(&instance.declaration) {
                let kind =
                    StandardTrait::from_id(&instance.declaration).expect("standard contract");
                if !kind.host_implementable()
                    && matches!(table.for_type, crate::module::abi::AbiType::Host(_))
                    || !table.native_bridge
                        && !kind.conversion()
                        && !matches!(
                            table.for_type,
                            crate::module::abi::AbiType::Struct(_)
                                | crate::module::abi::AbiType::Enum(_)
                                | crate::module::abi::AbiType::Host(_)
                        )
                    || !verify::interface_contract_matches(table, contract, &Default::default())
                {
                    return Err(BytecodeVerificationError::InvalidInterfaceTable);
                }
                continue;
            }
            if instance.declaration.module == module.identity {
                continue;
            }
            if instance.declaration.path.len() != 1
                || instance.declaration.path[0].kind != DefinitionKind::Trait
                || instance.declaration.path[0].occurrence != 0
            {
                return Err(BytecodeVerificationError::InvalidInterfaceTable);
            }
            let Some((owner_index, owner)) = program
                .modules
                .iter()
                .enumerate()
                .find(|(_, owner)| owner.identity == instance.declaration.module)
            else {
                return Err(BytecodeVerificationError::InvalidInterfaceTable);
            };
            if !reachable.contains(&ModuleRef::new(owner_index)) {
                return Err(BytecodeVerificationError::InvalidInterfaceTable);
            }
            let Some(trait_name) = instance.declaration.path.last().map(|part| &part.name) else {
                return Err(BytecodeVerificationError::InvalidInterfaceTable);
            };
            let Some(PublicAbiItem::Trait(interface)) = owner
                .public_items
                .iter()
                .find(|item| matches!(item, crate::module::PublicAbiItem::Trait(interface) if &interface.name == trait_name))
            else {
                return Err(BytecodeVerificationError::InvalidInterfaceTable);
            };
            if !verify::interface_contract_matches(table, interface, &Default::default()) {
                return Err(BytecodeVerificationError::InvalidInterfaceTable);
            }
        }
        if !trait_bounds::trait_bounds_match(module, &closure, Some(program)) {
            return Err(BytecodeVerificationError::InvalidHostInterface(
                "trait output or host bound has no unique valid implementation".into(),
            ));
        }
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
    Ok(())
}

impl BytecodeProgram {
    pub fn dependency_fingerprints(&self) -> Vec<DependencyFingerprint> {
        self.modules
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != self.root.index())
            .map(|(_, module)| DependencyFingerprint {
                module_id: module.identity.clone(),
                fingerprint: ArtifactFingerprint::of_serialized(module),
            })
            .collect()
    }
}
