//! Checked public namespace edges, canonical types and authored function declarations.

use crate::{
    hir::item::function::FunctionKind,
    imports::{
        BindingOrigin, NamespaceId, ResolvedTarget, SourceDeclRef, SourceItem, SourceUnit,
        catalog::LookupResult,
    },
    program::{CheckedProgram, ProgramCheckError},
    resolver::table::NameTable,
    typeck::FunctionImplementation,
    types::TypeId,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        MAX_IDENTITY_PATH_SEGMENTS, ModuleIdentity,
        table::{DefinitionId, module::ModuleHandle},
    },
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
    pub types: BTreeMap<String, TypeExport>,
}

/// Checked meaning of a public type binding, independent of its re-export spelling.
#[derive(Debug, Clone)]
pub enum TypeExport {
    /// Canonical source struct, enum, trait or opaque declaration identity.
    Declaration(DefinitionId),
    /// A zero-parameter alias normalized in its declaring module, not the importer.
    Closed(TypeId),
    /// A rejection-only name; erased parameters still make an alias open.
    OpenAlias { arity: u32 },
}

/// A finite namespace graph rooted at the selected Program module.
///
/// `pub use crate::internal::calculate as damage` records `damage` as a leaf
/// pointing to `internal`'s original function. Public module aliases preserve
/// cycles as edges; path lookup is bounded and never expands all possible paths.
/// Function leaves are script free-function declarations, including unused generics.
/// They do not imply an executable instance; member binding is separate. Type leaves
/// preserve declarations, normalized closed aliases and rejection-only open aliases.
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
                        ResolvedTarget::Source(source) if script_function(program, &source) => {
                            exported.functions.insert(name.as_str().into(), source);
                        }
                        ResolvedTarget::Source(source) if category == NameNamespace::Type => {
                            exported
                                .types
                                .insert(spelling.into(), checked_type_export(program, &source)?);
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
                && script_function(program, &source)
                && program
                    .source_function(&source)
                    .is_some_and(|(_, signature)| signature.generic_params.is_empty()))
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

fn checked_type_export(
    program: &CheckedProgram,
    source: &SourceDeclRef,
) -> Result<TypeExport, ProgramCheckError> {
    let invalid = || ProgramCheckError::Exports("missing checked public type declaration".into());
    let module = program
        .by_file
        .get(&source.unit.file)
        .map(|index| &program.modules[*index])
        .filter(|module| source.unit.matches(&module.lowered, module.definitions()))
        .ok_or_else(invalid)?;
    let id = module
        .declarations
        .definition(source.item.local().ok_or_else(invalid)?)
        .copied()
        .ok_or_else(invalid)?;
    if matches!(source.item, SourceItem::TypeAlias(_)) {
        let alias = module
            .declarations
            .imported_types()
            .alias(&id)
            .ok_or_else(invalid)?;
        if alias.error.is_some() {
            return Err(invalid());
        }
        if !alias.parameters.is_empty() {
            return Ok(TypeExport::OpenAlias {
                arity: u32::try_from(alias.parameters.len()).map_err(|_| invalid())?,
            });
        }
        let target = module
            .aggregates
            .normalize_type(module.definitions(), &alias.target);
        let target = module
            .aggregates
            .canonical_value_type(module.definitions(), &target);
        if !target.is_concrete() {
            return Err(invalid());
        }
        Ok(TypeExport::Closed(target))
    } else {
        Ok(TypeExport::Declaration(id))
    }
}

fn script_function(program: &CheckedProgram, source: &SourceDeclRef) -> bool {
    program
        .source_function(source)
        .is_some_and(|(module, signature)| {
            module.lowered.module.functions[signature.id.index()].kind == FunctionKind::User
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
