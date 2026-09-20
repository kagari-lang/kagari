//! Declaration queries do not depend on the runtime or invoke host callbacks.
use kagari_common::host_interface::{
    HostFunctionDeclaration, HostInterface, HostInterfaceError, HostTypeDeclaration, HostValueType,
};
use std::{collections::HashMap, sync::Arc};

use crate::types::{BuiltinType, TypeId};

#[cfg(test)]
mod facade_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod type_tests;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HostTypeId {
    revision: u64,
    index: usize,
}

#[derive(Debug)]
pub struct HostDeclarations {
    revision: u64,
    interface: HostInterface,
    paths: HashMap<String, HostFunctionId>,
    methods: HashMap<(kagari_common::identity::DefinitionId, String), HostFunctionId>,
    type_paths: HashMap<String, HostTypeId>,
    type_identities: HashMap<kagari_common::identity::DefinitionId, HostTypeId>,
    modules: Vec<String>,
}

impl HostDeclarations {
    pub fn new(mut interface: HostInterface) -> Result<Arc<Self>, HostInterfaceError> {
        interface.validate()?;
        let mut present = interface
            .functions
            .iter()
            .map(|f| f.id.clone())
            .collect::<std::collections::HashSet<_>>();
        for owner in &interface.types {
            for method in &owner.methods {
                if present.insert(method.id.clone()) {
                    interface.functions.push(owner.method_contract(&method.id)?);
                }
            }
        }
        interface.validate()?;
        if interface
            .functions
            .iter()
            .map(|function| &function.symbol)
            .chain(interface.types.iter().map(|ty| &ty.symbol))
            .any(|symbol| {
                symbol.split('.').any(|segment| {
                    let mut chars = segment.chars();
                    !chars
                        .next()
                        .is_some_and(|ch| ch == '_' || ch.is_alphabetic())
                        || !chars.all(|ch| ch == '_' || ch.is_alphanumeric())
                }) || symbol.split('.').next() == Some("std")
            })
        {
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
            .filter(|(_, declaration)| declaration.method_owner().is_none())
            .map(|(index, declaration)| {
                (
                    declaration.symbol.replace('.', "::"),
                    HostFunctionId { revision, index },
                )
            })
            .collect();
        let methods = interface
            .functions
            .iter()
            .enumerate()
            .filter_map(|(index, declaration)| {
                Some((
                    (
                        declaration.method_owner()?,
                        declaration.id.path.last()?.name.clone(),
                    ),
                    HostFunctionId { revision, index },
                ))
            })
            .collect();
        let mut modules = std::collections::BTreeSet::new();
        let type_paths: HashMap<_, _> = interface
            .types
            .iter()
            .enumerate()
            .map(|(index, declaration)| {
                (
                    declaration.symbol.replace('.', "::"),
                    HostTypeId { revision, index },
                )
            })
            .collect();
        let type_identities = interface
            .types
            .iter()
            .enumerate()
            .map(|(index, declaration)| (declaration.id.clone(), HostTypeId { revision, index }))
            .collect();
        for symbol in interface
            .functions
            .iter()
            .filter(|function| function.method_owner().is_none())
            .map(|function| &function.symbol)
            .chain(interface.types.iter().map(|ty| &ty.symbol))
        {
            let path = symbol.replace('.', "::");
            for (offset, _) in path.match_indices("::") {
                modules.insert(path[..offset].to_owned());
            }
        }
        if modules
            .iter()
            .any(|module| paths.contains_key(module) || type_paths.contains_key(module))
        {
            return Err(HostInterfaceError::DuplicateDeclaration);
        }
        Ok(Arc::new(Self {
            revision,
            interface,
            paths,
            methods,
            type_paths,
            type_identities,
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
    pub fn method(
        &self,
        owner: &kagari_common::identity::DefinitionId,
        name: &str,
    ) -> Option<HostFunctionId> {
        self.methods.get(&(owner.clone(), name.to_owned())).copied()
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
    pub fn function(&self, id: HostFunctionId) -> Option<&HostFunctionDeclaration> {
        (id.revision == self.revision)
            .then(|| self.interface.functions.get(id.index))
            .flatten()
    }
    pub(crate) fn resolve_name_in(
        &self,
        module: HostModuleId,
        path: &str,
    ) -> Option<crate::resolver::ResolvedName> {
        if module.revision != self.revision {
            return None;
        }
        self.resolve_name(&format!("{}::{path}", self.modules.get(module.index)?))
    }
    pub(crate) fn resolve_name(&self, path: &str) -> Option<crate::resolver::ResolvedName> {
        self.resolve(path)
            .map(crate::resolver::ResolvedName::HostFunction)
            .or_else(|| {
                self.resolve_type(path)
                    .map(crate::resolver::ResolvedName::HostType)
            })
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn resolve_type(&self, path: &str) -> Option<HostTypeId> {
        self.type_paths.get(path).copied()
    }
    pub fn nominal_type(
        &self,
        declaration: &kagari_common::identity::DefinitionId,
    ) -> Option<HostTypeId> {
        self.type_identities.get(declaration).copied()
    }
    pub fn type_declaration(&self, id: HostTypeId) -> Option<&HostTypeDeclaration> {
        (id.revision == self.revision)
            .then(|| self.interface.types.get(id.index))
            .flatten()
    }
}

pub(crate) fn signature_type(ty: &HostValueType) -> TypeId {
    match ty {
        HostValueType::Tuple(types) => TypeId::Tuple(types.iter().map(signature_type).collect()),
        HostValueType::Array(element) => TypeId::Array(Box::new(signature_type(element))),
        HostValueType::Map { key, value } => TypeId::Map {
            key: Box::new(signature_type(key)),
            value: Box::new(signature_type(value)),
        },
        HostValueType::Set(element) => TypeId::Set(Box::new(signature_type(element))),
        HostValueType::Option(element) => TypeId::StandardEnum {
            kind: crate::builtin::surface::StandardEnum::Option,
            args: vec![signature_type(element)],
        },
        HostValueType::Result { ok, error } => TypeId::StandardEnum {
            kind: crate::builtin::surface::StandardEnum::Result,
            args: vec![signature_type(ok), signature_type(error)],
        },
        HostValueType::Opaque(id) => TypeId::Host(id.clone()),
        scalar => TypeId::Builtin(match scalar {
            HostValueType::Unit => BuiltinType::Unit,
            HostValueType::Bool => BuiltinType::Bool,
            HostValueType::I32 => BuiltinType::I32,
            HostValueType::I64 => BuiltinType::I64,
            HostValueType::F32 => BuiltinType::F32,
            HostValueType::F64 => BuiltinType::F64,
            HostValueType::String => BuiltinType::String,
            _ => unreachable!("composite handled above"),
        }),
    }
}
