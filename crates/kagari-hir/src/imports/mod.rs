//! Immutable import syntax, scope bindings and qualified namespace identities.
use crate::{
    hir::ids::{
        ConstId, EnumId, FunctionId, HirArenaId, ModuleId, OpaqueTypeId, StructId, TraitId,
        VariantId,
    },
    host::{HostFunctionId, HostModuleId, HostTypeId},
    lower::LoweredModule,
    resolver::{resolved::ResolvedName, table::NameTable},
};
use kagari_common::identity::{DefinitionPath, ModuleIdentity, PackageId};
use kagari_source::{
    diagnostic::Diagnostic,
    identity::{FileId, FileSpan, Revision},
};
use kagari_types::{collection::CollectionAccess, visibility::Visibility};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::Arc,
};

#[cfg(test)]
mod aggregate_tests;
mod builder;
mod cache;
pub mod catalog;
pub mod functions;
#[cfg(test)]
mod provenance_tests;
#[cfg(test)]
mod signature_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod type_tests;
pub mod types;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SourceUnit {
    pub module: ModuleIdentity,
    pub file: FileId,
    pub revision: Revision,
    pub arena: HirArenaId,
}
impl SourceUnit {
    pub(crate) fn of(lowered: &LoweredModule) -> Self {
        Self {
            module: lowered.source.module_identity().clone(),
            file: lowered.source.id(),
            revision: lowered.source.revision(),
            arena: lowered.module.body.arena(),
        }
    }
    pub(crate) fn matches(&self, lowered: &LoweredModule) -> bool {
        *self == Self::of(lowered)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceItem {
    Function(FunctionId),
    Const(ConstId),
    Struct(StructId),
    Enum(EnumId),
    OpaqueType(OpaqueTypeId),
    Trait(TraitId),
    Variant(VariantId),
}
impl SourceItem {
    pub(crate) fn local(self) -> Option<ResolvedName> {
        Some(match self {
            Self::Function(id) => ResolvedName::Function(id),
            Self::Const(id) => ResolvedName::Const(id),
            Self::Struct(id) => ResolvedName::Struct(id),
            Self::Enum(id) => ResolvedName::Enum(id),
            Self::OpaqueType(id) => ResolvedName::OpaqueType(id),
            Self::Trait(id) => ResolvedName::Trait(id),
            Self::Variant(_) => return None,
        })
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SourceDeclRef {
    pub unit: SourceUnit,
    pub item: SourceItem,
}
impl SourceDeclRef {
    pub fn function(&self) -> Option<FunctionId> {
        match self.item {
            SourceItem::Function(id) => Some(id),
            _ => None,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NamespaceId {
    Module(SourceUnit),
    Associated(SourceDeclRef),
    Host(HostModuleId),
    InstalledPrefix(ModuleIdentity),
}
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ResolvedTarget {
    Namespace(NamespaceId),
    Source(SourceDeclRef),
    HostFunction(HostFunctionId),
    HostType(HostTypeId),
}
impl ResolvedTarget {
    pub(crate) fn resolved(&self, unit: Option<&SourceUnit>) -> ResolvedName {
        match self {
            Self::Source(source) => {
                if Some(&source.unit) == unit
                    && let Some(local) = source.item.local()
                {
                    return local;
                }
                ResolvedName::Source(source.clone())
            }
            Self::Namespace(ns) => ResolvedName::Namespace(ns.clone()),
            Self::HostFunction(id) => ResolvedName::HostFunction(*id),
            Self::HostType(id) => ResolvedName::HostType(*id),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DirectiveId {
    pub unit: SourceUnit,
    pub slot: u32,
}
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LocalName(String);
impl LocalName {
    pub(crate) fn new(name: &str) -> Option<Self> {
        let mut chars = name.chars();
        let first = chars.next()?;
        ((first == '_' || first.is_alphabetic()) && chars.all(|c| c == '_' || c.is_alphanumeric()))
            .then(|| Self(name.into()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportKind {
    Named { alias: Option<LocalName> },
    Glob,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirectiveResolution {
    Pending,
    Resolved(ResolvedTarget),
    Unresolved,
    Ambiguous,
}
impl DirectiveResolution {
    pub fn target(&self) -> Option<&ResolvedTarget> {
        if let Self::Resolved(target) = self {
            Some(target)
        } else {
            None
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportDirective {
    pub id: DirectiveId,
    pub path: String,
    pub kind: ImportKind,
    pub span: FileSpan,
    pub root_span: FileSpan,
    pub visibility: Visibility,
    pub resolution: DirectiveResolution,
    pub direct_dependencies: BTreeSet<ModuleIdentity>,
}
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BindingOrigin {
    Declaration(SourceDeclRef),
    ModuleDeclaration { unit: SourceUnit, module: ModuleId },
    NamedImport(DirectiveId),
    GlobImport(DirectiveId),
    Package(PackageId),
    Prelude(NamespaceId),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingCandidate {
    pub target: Option<ResolvedTarget>,
    pub origin: BindingOrigin,
    pub owner: ModuleIdentity,
    pub visibility: Visibility,
    pub location: Option<FileSpan>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NameEntry {
    pub strong: Vec<BindingCandidate>,
    pub globs: Vec<BindingCandidate>,
    pub implicit: Vec<BindingCandidate>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModuleImportFacts {
    pub(crate) path_hits: Vec<(FileSpan, catalog::LookupHit)>,
    pub directives: Vec<ImportDirective>,
    pub scope: Arc<NameTable>,
    pub diagnostics: Vec<Diagnostic>,
    pub dependencies: Vec<ModuleIdentity>,
    pub(crate) array_interfaces: BTreeMap<CollectionAccess, DefinitionPath>,
}
#[derive(Debug, Clone)]
pub struct ModuleNode {
    pub file: FileId,
    pub revision: Revision,
    pub imports: Arc<ModuleImportFacts>,
}
impl ModuleNode {
    pub fn dependencies(&self) -> &[ModuleIdentity] {
        &self.imports.dependencies
    }
}
#[derive(Debug, Clone, Default)]
pub struct ModuleGraph {
    nodes: BTreeMap<ModuleIdentity, ModuleNode>,
    source_facts: HashMap<SourceUnit, Arc<ModuleImportFacts>>,
    pub catalog: Arc<catalog::NamespaceCatalog>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleOrderError {
    Missing(ModuleIdentity),
    InvalidImports(ModuleIdentity),
    Cancelled,
}
