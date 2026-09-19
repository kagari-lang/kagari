use super::{BytecodeInstruction, BytecodeModule, BytecodeVerificationError, CallTarget};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

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
/// scoped to this program; modules retain their own initialization and metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BytecodeProgram {
    pub root: ModuleRef,
    pub modules: Vec<BytecodeModule>,
}

pub fn verify_program(program: &BytecodeProgram) -> Result<(), BytecodeVerificationError> {
    let invalid = || BytecodeVerificationError::InvalidProgramGraph;
    if program.modules.len() > u32::MAX as usize || program.root.index() >= program.modules.len() {
        return Err(invalid());
    }
    let mut identities = HashSet::new();
    let mut layouts = HashMap::new();
    let mut enum_layouts = HashMap::new();
    let mut hosts = HashMap::new();
    let mut symbols = HashMap::new();
    for (index, module) in program.modules.iter().enumerate() {
        let mut dependencies = HashSet::new();
        if !identities.insert(&module.identity)
            || module
                .dependencies
                .iter()
                .any(|dependency| dependency.index() >= index || !dependencies.insert(*dependency))
        {
            return Err(invalid());
        }
        for layout in &module.structures {
            if let Some(previous) = layouts.insert((&layout.declaration, &layout.arguments), layout)
                && previous != layout
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
        super::verifier::verify_module_with_program(module, Some(program))?;
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
            if !crate::module::layout::enum_abi_matches(
                std::slice::from_ref(layout),
                &owner.identity,
                std::slice::from_ref(template),
            ) {
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
        for instruction in module
            .functions
            .iter()
            .flat_map(|function| &function.instructions)
        {
            if let BytecodeInstruction::Call {
                callee: CallTarget::ModuleFunction { module: target, .. },
                ..
            } = instruction
                && !reachable.contains(target)
            {
                return Err(invalid());
            }
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
    pub fn dependency_fingerprints(&self) -> Vec<super::DependencyFingerprint> {
        self.modules
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != self.root.index())
            .map(|(_, module)| super::DependencyFingerprint {
                module_id: module.identity.clone(),
                fingerprint: super::ArtifactFingerprint::of_serialized(module),
            })
            .collect()
    }
}
