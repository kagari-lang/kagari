//! Checked public namespace edges and concrete source function leaves.

use crate::{
    hir::item::function::FunctionKind,
    imports::{
        BindingOrigin, NamespaceId, ResolvedTarget, SourceDeclRef, SourceItem, SourceUnit,
        catalog::LookupResult,
    },
    program::{CheckedProgram, ProgramCheckError},
    resolver::table::NameTable,
    typeck::FunctionImplementation,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{MAX_IDENTITY_PATH_SEGMENTS, ModuleIdentity, table::module::ModuleHandle},
};
use kagari_types::{declaration::names::NameNamespace, visibility::Visibility};
use std::collections::{BTreeMap, HashSet};

const MAX_EXPORT_RECORDS: usize = 65_536;

/// One public namespace, retaining independent type/module and function spellings.
/// A module alias points to its canonical namespace rather than copying members.
#[derive(Debug, Clone, Default)]
pub struct ExportNamespace {
    pub modules: BTreeMap<String, ModuleHandle>,
    pub functions: BTreeMap<String, SourceDeclRef>,
}

/// A finite namespace graph rooted at the selected Program module.
///
/// `pub use crate::internal::calculate as damage` records `damage` as a leaf
/// pointing to `internal`'s original function. Public module aliases preserve
/// cycles as edges; path lookup is bounded and never expands all possible paths.
/// Leaves are non-generic script free functions. Generic/native APIs and associated
/// methods use concrete free-function wrappers on this path; member binding is separate.
#[derive(Debug, Clone, Default)]
pub struct ProgramExports {
    namespaces: BTreeMap<ModuleHandle, ExportNamespace>,
    entry: Option<SourceDeclRef>,
}

impl ProgramExports {
    /// Borrows checked public names; source units remain owned by the checked Program.
    /// Keys use context-local order. Resolve through the root catalog and sort portable
    /// identities when emitting an externally ordered representation.
    pub fn namespaces(&self) -> &BTreeMap<ModuleHandle, ExportNamespace> {
        &self.namespaces
    }

    /// The root's zero-argument, non-generic script `main`, independently of visibility.
    /// This is a separate CLI capability and is absent from public lookup when private.
    pub fn entry(&self) -> Option<&SourceDeclRef> {
        self.entry.as_ref()
    }

    /// Resolves a relative public function path, including module/re-export aliases.
    /// Empty components, absent/private names and paths over the identity bound fail.
    pub fn function(&self, root: &ModuleHandle, path: &str) -> Option<&SourceDeclRef> {
        let mut parts = path.split("::").peekable();
        let mut current = root;
        for _ in 0..MAX_IDENTITY_PATH_SEGMENTS {
            let name = parts.next()?;
            if name.is_empty() {
                return None;
            }
            let namespace = self.namespaces.get(current)?;
            if parts.peek().is_none() {
                return namespace.functions.get(name);
            }
            current = namespace.modules.get(name)?;
        }
        None
    }

    pub(super) fn collect(
        program: &CheckedProgram,
        cancel: &CancellationToken,
    ) -> Result<Self, ProgramCheckError> {
        let root = program.root();
        let catalog = &root.names.catalog;
        let mut pending = vec![NamespaceId::Module(SourceUnit::of(
            &root.lowered,
            &catalog.definitions,
        ))];
        let mut seen = HashSet::new();
        let mut result = Self::default();
        let mut records = 0usize;
        while let Some(namespace) = pending.pop() {
            cancel.check().map_err(|_| ProgramCheckError::Cancelled)?;
            if !seen.insert(namespace.clone()) {
                continue;
            }
            let Some(identity) = module_handle(&namespace) else {
                continue;
            };
            let table = catalog
                .namespaces
                .get(&namespace)
                .ok_or(ProgramCheckError::Exports(format!(
                    "missing public namespace {}",
                    catalog.identity(identity)
                )))?;
            let mut exported = ExportNamespace::default();
            for name in table.names.entries.keys() {
                let spelling = name.as_str();
                for category in NameNamespace::ALL {
                    cancel.check().map_err(|_| ProgramCheckError::Cancelled)?;
                    let Some(target) = public_target(
                        &table.names,
                        catalog.identity(identity),
                        spelling,
                        category,
                    )?
                    else {
                        continue;
                    };
                    match target {
                        ResolvedTarget::Source(source)
                            if concrete_script_function(program, &source) =>
                        {
                            exported.functions.insert(name.as_str().into(), source);
                        }
                        ResolvedTarget::Namespace(child) => {
                            if let Some(target) = module_handle(&child) {
                                exported.modules.insert(name.as_str().into(), target);
                                pending.push(child);
                            }
                        }
                        _ => continue,
                    }
                    records += 1;
                    if records > MAX_EXPORT_RECORDS {
                        return Err(ProgramCheckError::Exports(
                            "public export record limit exceeded".into(),
                        ));
                    }
                }
            }
            result.namespaces.insert(identity, exported);
        }
        result.entry = root.lowered.module.functions.iter().find_map(|function| {
            let source = SourceDeclRef {
                unit: SourceUnit::of(&root.lowered, &catalog.definitions),
                item: SourceItem::Function(function.id),
            };
            (function.name == "main"
                && function.params.is_empty()
                && concrete_script_function(program, &source))
            .then_some(source)
        });
        cancel.check().map_err(|_| ProgramCheckError::Cancelled)?;
        Ok(result)
    }
}

fn public_target(
    names: &NameTable,
    owner: &ModuleIdentity,
    name: &str,
    category: NameNamespace,
) -> Result<Option<ResolvedTarget>, ProgramCheckError> {
    let Some((candidates, strong)) = names.candidates(name, category) else {
        return Ok(None);
    };
    let public = candidates
        .iter()
        .filter(|candidate| {
            candidate.visibility == Visibility::Public
                && !matches!(
                    candidate.origin,
                    BindingOrigin::Package(_) | BindingOrigin::Prelude(_)
                )
        })
        .cloned()
        .collect::<Vec<_>>();
    if public.is_empty() {
        return Ok(None);
    }
    if strong && candidates.len() > 1 {
        return Err(ProgramCheckError::Exports(format!(
            "ambiguous public export {owner}::{name}"
        )));
    }
    match NameTable::select(&public, strong) {
        LookupResult::Found(hit) => Ok(Some(hit.target)),
        _ => Err(ProgramCheckError::Exports(format!(
            "unresolved or ambiguous public export {owner}::{name}"
        ))),
    }
}

fn concrete_script_function(program: &CheckedProgram, source: &SourceDeclRef) -> bool {
    program
        .source_function(source)
        .is_some_and(|(module, signature)| {
            module.lowered.module.functions[signature.id.index()].kind == FunctionKind::User
                && signature.generic_params.is_empty()
                && matches!(signature.implementation, FunctionImplementation::Script)
        })
}

fn module_handle(namespace: &NamespaceId) -> Option<ModuleHandle> {
    match namespace {
        NamespaceId::Module(source) => Some(source.module),
        NamespaceId::InstalledPrefix(identity) => Some(*identity),
        NamespaceId::Associated(_) => None,
    }
}
