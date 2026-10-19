//! Source-independent public paths and the separate root entry capability.

mod mapping;
pub mod wire;

use crate::{slots::FunctionTypeFacts, types::PublicItem};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, DefinitionPath, MAX_IDENTITY_PATH_SEGMENTS, ModuleIdentity,
        mapping::{DefinitionMappingError, check_cancel},
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_types::{callable::CallableImplementation, declaration::NativeDeclaration};
use std::collections::{BTreeMap, HashSet};

pub const MAX_EXPORT_RECORDS: usize = 65_536;

/// Public module aliases and concrete free-function declarations in one namespace.
/// The two name spaces are independent: `api` may name both a module and a function.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExportNamespace {
    pub modules: BTreeMap<String, ModuleIdentity>,
    pub functions: BTreeMap<String, DefinitionId>,
}

/// Editable export facts, owned and verified together with an executable Program.
///
/// For `pub use internal::calculate as damage`, the root's `damage` leaf contains
/// `calculate`'s canonical declaration ID. Module aliases retain graph edges, including
/// cycles. `entry` is a separate zero-argument root `main`, possibly private.
/// These facts grant no authority until the enclosing executable verifier checks them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProgramExports {
    pub namespaces: BTreeMap<ModuleIdentity, ExportNamespace>,
    pub entry: Option<DefinitionId>,
}

impl ProgramExports {
    /// Resolve a relative public path in a verified graph without expanding aliases.
    /// Empty components, private names and paths over the identity bound return `None`.
    pub fn function(&self, root: &ModuleIdentity, path: &str) -> Option<DefinitionId> {
        let mut parts = path.split("::").peekable();
        let mut current = root;
        for _ in 0..MAX_IDENTITY_PATH_SEGMENTS {
            let name = parts.next()?;
            if name.is_empty() {
                return None;
            }
            let namespace = self.namespaces.get(current)?;
            if parts.peek().is_none() {
                return namespace.functions.get(name).copied();
            }
            current = namespace.modules.get(name)?;
        }
        None
    }

    /// Check graph bounds, reachable namespaces and canonical free-function identities.
    /// The executable owner must additionally prove each leaf has a concrete public
    /// script body, and that the separate entry has a zero-argument script body.
    /// An empty graph explicitly exposes no public paths; it can still carry an entry.
    pub fn validate(
        &self,
        root: &ModuleIdentity,
        definitions: &DefinitionTable,
        cancel: &CancellationToken,
        mut executable: impl FnMut(DefinitionId, bool) -> bool,
    ) -> Result<(), DefinitionMappingError> {
        self.check_shape(cancel)?;
        let invalid = DefinitionMappingError::InvalidContract;
        let mut seen = HashSet::new();
        let mut pending = Vec::new();
        if !self.namespaces.is_empty() {
            pending.push(root);
        }
        while let Some(identity) = pending.pop() {
            check_cancel(cancel)?;
            if !seen.insert(identity) {
                continue;
            }
            let namespace = self.namespaces.get(identity).ok_or(invalid)?;
            pending.extend(namespace.modules.values());
            for id in namespace.functions.values() {
                check_cancel(cancel)?;
                free_function(*id, definitions)?;
                if !executable(*id, false) {
                    return Err(invalid);
                }
            }
        }
        if seen.len() != self.namespaces.len() {
            return Err(invalid);
        }
        if let Some(entry) = self.entry {
            let path = free_function(entry, definitions)?;
            if &path.module != root || path.path[0].name != "main" || !executable(entry, true) {
                return Err(invalid);
            }
        }
        check_cancel(cancel)
    }

    pub(super) fn check_shape(
        &self,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        let mut count = self.namespaces.len() + usize::from(self.entry.is_some());
        if count > MAX_EXPORT_RECORDS {
            return Err(DefinitionMappingError::LimitExceeded);
        }
        for (identity, namespace) in &self.namespaces {
            check_cancel(cancel)?;
            count = count
                .checked_add(namespace.modules.len())
                .and_then(|count| count.checked_add(namespace.functions.len()))
                .filter(|count| *count <= MAX_EXPORT_RECORDS)
                .ok_or(DefinitionMappingError::LimitExceeded)?;
            if !identity.within_path_limit()
                || identity.package.0.is_empty()
                || identity.path.iter().any(|name| !valid_name(name))
                || namespace
                    .modules
                    .keys()
                    .chain(namespace.functions.keys())
                    .any(|name| !valid_name(name))
            {
                return Err(DefinitionMappingError::InvalidContract);
            }
        }
        Ok(())
    }
}

fn valid_name(name: &str) -> bool {
    !name.is_empty() && !name.contains("::")
}

/// Match a public, non-generic script declaration against its semantic signature.
/// Executable owners additionally check body existence and physical layout.
pub fn public_function_matches(
    id: DefinitionId,
    owner: &ModuleIdentity,
    items: &[PublicItem],
    semantic: &impl FunctionTypeFacts,
    definitions: &DefinitionTable,
) -> bool {
    let Ok(path) = free_function(id, definitions) else {
        return false;
    };
    let Some(function) = items.iter().find_map(|item| match item {
        PublicItem::Function(function) if function.name == path.path[0].name => Some(function),
        _ => None,
    }) else {
        return false;
    };
    &path.module == owner
        && semantic.generic().is_none()
        && function.generic_params.is_empty()
        && function.implementation == CallableImplementation::Script
        && function.params.len() == semantic.params().len()
        && function
            .params
            .iter()
            .enumerate()
            .all(|(index, parameter)| semantic.params().get(&index) == Some(&parameter.ty))
        && semantic.result() == Some(&function.return_type)
}

fn free_function(
    id: DefinitionId,
    definitions: &DefinitionTable,
) -> Result<DefinitionPath, DefinitionMappingError> {
    let path = definitions.resolve(id)?.to_path();
    if path.path.len() != 1
        || path.path[0].kind != DefinitionKind::Function
        || path.path[0].occurrence != 0
    {
        return Err(DefinitionMappingError::InvalidContract);
    }
    Ok(path)
}

/// Check the semantic contract of the separate root entry's script body.
/// The graph validator checks its canonical root `main` identity; the executable
/// owner checks its physical parameter count. Native declarations never grant this entry.
pub fn entry_function_matches(
    id: DefinitionId,
    items: &[PublicItem],
    native: &[NativeDeclaration],
    semantic: &impl FunctionTypeFacts,
    definitions: &DefinitionTable,
) -> bool {
    let Ok(path) = free_function(id, definitions) else {
        return false;
    };
    semantic.generic().is_none()
        && semantic.params().is_empty()
        && semantic.result().is_some()
        && !native.iter().any(|record| record.declaration == id)
        && (!items.iter().any(
            |item| matches!(item, PublicItem::Function(record) if record.name == path.path[0].name),
        ) || public_function_matches(id, &path.module, items, semantic, definitions))
}
