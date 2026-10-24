//! Source-independent public paths and the separate root entry capability.

mod mapping;
pub mod path;
pub mod type_validation;
pub mod wire;

use crate::{
    exports::path::{PublicPathKind, PublicPathTarget},
    slots::FunctionTypeFacts,
    types::{
        PublicItem,
        named::{NamedContract, NamedDeclaration},
    },
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, DefinitionPath, ModuleIdentity,
        mapping::{DefinitionMappingError, check_cancel},
        scoped::DefinitionReferences,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{NativeDeclaration, verify::concrete_type_valid},
    decode_limits::MAX_NESTED_RECORDS,
    ty::Ty,
    visibility::Visibility,
};
use std::collections::{BTreeMap, HashSet};

pub const MAX_EXPORT_RECORDS: usize = 65_536;

/// Public module aliases, type exports and script free-function declarations.
/// Generic function leaves grant declaration lookup, not executable instance authority.
/// Types and modules share a namespace; functions occupy an independent namespace.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExportNamespace {
    pub modules: BTreeMap<String, ModuleIdentity>,
    pub functions: BTreeMap<String, DefinitionId>,
    pub types: BTreeMap<String, TypeExport>,
}

/// A public type name without source spelling or executable layout authority.
///
/// For `pub type IntCell = hidden::Cell<i32>`, `Closed` retains the normalized
/// `Cell<i32>` application, not an alias identity. `pub type Erased<T> = User`
/// remains `OpenAlias { arity: 1 }` even though its target does not use `T`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeExport {
    /// Canonical struct, enum, trait, host or native declaration, including renames.
    /// The defining public contract owns its kind, parameters and bounds.
    Declaration(DefinitionId),
    /// Fully normalized target of an alias with no declared alias parameters.
    Closed(Ty),
    /// Rejection-only marker; grants no binder, type application or layout authority.
    /// Arity must be positive and at most `MAX_NESTED_RECORDS`.
    OpenAlias { arity: u32 },
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
    /// Invalid, private, missing and wrong-kind paths return `None`.
    pub fn function(&self, root: &ModuleIdentity, path: &str) -> Option<DefinitionId> {
        match self
            .resolve_public_path(root, path, PublicPathKind::Function)
            .ok()?
        {
            PublicPathTarget::Function(declaration) => Some(declaration),
            _ => None,
        }
    }

    /// Check graph bounds, reachability and structural function/type leaf validity.
    /// The function callback must prove concrete public script bodies or checked public
    /// generic declarations, and the separate zero-argument executable entry.
    /// The type callback must prove retained public declaration,
    /// application/bound and executable layout authority, not merely ID membership.
    /// It receives every structurally valid type leaf, including rejection-only aliases.
    /// Invalid leaves or rejected authority return `InvalidContract`; identity,
    /// cancellation and bounded-shape failures retain their corresponding errors.
    /// An empty graph explicitly exposes no public paths; it can still carry an entry.
    pub fn validate(
        &self,
        root: &ModuleIdentity,
        definitions: &DefinitionTable,
        cancel: &CancellationToken,
        mut executable: impl FnMut(DefinitionId, bool) -> bool,
        mut type_export: impl FnMut(&TypeExport) -> bool,
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
            for export in namespace.types.values() {
                check_cancel(cancel)?;
                match export {
                    TypeExport::Declaration(id) => {
                        let path = definitions.resolve(*id)?.to_path();
                        if path.path.len() != 1
                            || path.path[0].occurrence != 0
                            || !matches!(
                                path.path[0].kind,
                                DefinitionKind::Struct
                                    | DefinitionKind::Enum
                                    | DefinitionKind::Trait
                                    | DefinitionKind::AssociatedType
                            )
                        {
                            return Err(invalid);
                        }
                    }
                    TypeExport::Closed(ty) => {
                        let valid =
                            ty.is_concrete() && concrete_type_valid(ty, cancel, definitions);
                        check_cancel(cancel)?;
                        if !valid {
                            return Err(invalid);
                        }
                    }
                    TypeExport::OpenAlias { .. } => {}
                }
                if !type_export(export) {
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
                .and_then(|count| count.checked_add(namespace.types.len()))
                .filter(|count| *count <= MAX_EXPORT_RECORDS)
                .ok_or(DefinitionMappingError::LimitExceeded)?;
            if !identity.within_path_limit()
                || identity.package.0.is_empty()
                || identity.path.iter().any(|name| !valid_name(name))
                || namespace
                    .modules
                    .keys()
                    .chain(namespace.functions.keys())
                    .chain(namespace.types.keys())
                    .any(|name| !valid_name(name))
                || namespace
                    .types
                    .keys()
                    .any(|name| namespace.modules.contains_key(name))
            {
                return Err(DefinitionMappingError::InvalidContract);
            }
            for export in namespace.types.values() {
                check_cancel(cancel)?;
                match export {
                    TypeExport::Closed(ty) => {
                        ty.visit_definition_ids(&mut |_| Ok(()), cancel)?;
                    }
                    TypeExport::OpenAlias { arity }
                        if *arity == 0 || *arity as usize > MAX_NESTED_RECORDS =>
                    {
                        return Err(DefinitionMappingError::InvalidContract);
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }
}

fn valid_name(name: &str) -> bool {
    !name.is_empty() && !name.contains("::")
}

/// Validate a generic function export against its owning public and named contracts.
/// This grants declaration lookup without requiring or fabricating a specialization.
/// Callers must first validate the module's public/named inventory and still require
/// executable evidence for calls and the separate root entry capability.
pub fn public_generic_function_matches(
    record: &NamedDeclaration,
    owner: &ModuleIdentity,
    items: &[PublicItem],
    definitions: &DefinitionTable,
) -> bool {
    let Ok(path) = free_function(record.declaration, definitions) else {
        return false;
    };
    let NamedContract::Function(function) = &record.contract else {
        return false;
    };
    &path.module == owner
        && record.visibility == Visibility::Public
        && path.path[0].name == function.name
        && !function.generic_params.is_empty()
        && function.implementation == CallableImplementation::Script
        && items
            .iter()
            .any(|item| matches!(item, PublicItem::Function(public) if public == function))
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
