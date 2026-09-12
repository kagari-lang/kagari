//! Declaration queries do not depend on the runtime or invoke host callbacks.
use kagari_common::host_interface::{
    HostFunctionDeclaration, HostInterface, HostInterfaceError, HostValueType,
};
use std::{collections::HashMap, sync::Arc};

use crate::types::{BuiltinType, TypeId};

#[cfg(test)]
mod tests;

/// Scoped to one immutable declaration input, never a runtime binding slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HostFunctionId {
    revision: u64,
    index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HostModuleId {
    revision: u64,
    index: usize,
}

#[derive(Debug)]
pub struct HostDeclarations {
    revision: u64,
    interface: HostInterface,
    paths: HashMap<String, HostFunctionId>,
    modules: Vec<String>,
}

impl HostDeclarations {
    pub fn new(interface: HostInterface) -> Result<Arc<Self>, HostInterfaceError> {
        interface.validate()?;
        if interface.functions.iter().any(|function| {
            function.symbol.split('.').any(|segment| {
                let mut chars = segment.chars();
                !chars
                    .next()
                    .is_some_and(|ch| ch == '_' || ch.is_alphabetic())
                    || !chars.all(|ch| ch == '_' || ch.is_alphanumeric())
            }) || function.symbol.split('.').next() == Some("std")
        }) {
            return Err(HostInterfaceError::InvalidDeclaration);
        }
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let revision = NEXT
            .fetch_update(
                std::sync::atomic::Ordering::Relaxed,
                std::sync::atomic::Ordering::Relaxed,
                |n| n.checked_add(1),
            )
            .expect("host declaration revision exhausted");
        let paths: HashMap<_, _> = interface
            .functions
            .iter()
            .enumerate()
            .map(|(index, declaration)| {
                (
                    declaration.symbol.replace('.', "::"),
                    HostFunctionId { revision, index },
                )
            })
            .collect();
        let mut modules = std::collections::BTreeSet::new();
        for declaration in &interface.functions {
            let path = declaration.symbol.replace('.', "::");
            for (offset, _) in path.match_indices("::") {
                modules.insert(path[..offset].to_owned());
            }
        }
        if modules.iter().any(|module| paths.contains_key(module)) {
            return Err(HostInterfaceError::DuplicateDeclaration);
        }
        Ok(Arc::new(Self {
            revision,
            interface,
            paths,
            modules: modules.into_iter().collect(),
        }))
    }

    pub fn empty() -> Arc<Self> {
        static EMPTY: std::sync::OnceLock<Arc<HostDeclarations>> = std::sync::OnceLock::new();
        EMPTY
            .get_or_init(|| Self::new(HostInterface::default()).unwrap())
            .clone()
    }

    pub fn resolve(&self, path: &str) -> Option<HostFunctionId> {
        self.paths.get(path).copied()
    }
    pub fn module(&self, path: &str) -> Option<HostModuleId> {
        self.modules
            .binary_search_by(|candidate| candidate.as_str().cmp(path))
            .ok()
            .map(|index| HostModuleId {
                revision: self.revision,
                index,
            })
    }
    pub fn resolve_in(&self, module: HostModuleId, path: &str) -> Option<HostFunctionId> {
        if module.revision != self.revision {
            return None;
        }
        self.resolve(&format!("{}::{path}", self.modules.get(module.index)?))
    }
    pub fn function(&self, id: HostFunctionId) -> Option<&HostFunctionDeclaration> {
        (id.revision == self.revision)
            .then(|| self.interface.functions.get(id.index))
            .flatten()
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
}

pub fn scalar_type(ty: &HostValueType) -> Option<TypeId> {
    Some(TypeId::Builtin(match ty {
        HostValueType::Unit => BuiltinType::Unit,
        HostValueType::Bool => BuiltinType::Bool,
        HostValueType::I32 => BuiltinType::I32,
        HostValueType::I64 => BuiltinType::I64,
        HostValueType::F32 => BuiltinType::F32,
        HostValueType::F64 => BuiltinType::F64,
        HostValueType::String => BuiltinType::String,
        HostValueType::Opaque(_) => return None,
    }))
}
