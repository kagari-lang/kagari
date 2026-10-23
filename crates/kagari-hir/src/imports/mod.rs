//! Immutable import syntax, scope bindings and qualified namespace identities.
use crate::{
    hir::ids::{
        ConstId, EnumId, FunctionId, HirArenaId, ModuleId, OpaqueTypeId, StaticId, StructId,
        TraitId, TypeAliasId, VariantId,
    },
    imports::bindings::{LookupOutcome, PerNamespace},
    lower::LoweredModule,
    resolver::{resolved::ResolvedName, table::NameTable},
};
use kagari_common::identity::{
    ModuleIdentity, PackageId,
    table::{DefinitionTable, DefinitionTableError, module::ModuleHandle},
};
use kagari_source::{
    diagnostic::Diagnostic,
    identity::{FileId, FileSpan, Revision},
};
use kagari_types::{declaration::names::NameNamespace, visibility::Visibility};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    sync::Arc,
};

#[cfg(test)]
mod aggregate_tests;
#[cfg(test)]
mod alias_tests;
pub mod bindings;
mod builder;
mod cache;
#[cfg(test)]
mod cache_tests;
pub mod catalog;
pub mod constants;
pub mod functions;
mod mapping;
#[cfg(test)]
mod provenance_tests;
mod resolve;
#[cfg(test)]
mod signature_tests;
pub mod solver;
#[cfg(test)]
mod solver_tests;
pub mod statics;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod type_tests;
pub mod types;

/// Identity of one logical source module in one immutable lowering.
///
/// `module` identifies a context-owned package/path root, `file` identifies the source-database entry,
/// `revision` distinguishes its text version, and `arena` distinguishes the HIR
/// allocation. Inline modules may share a physical origin while having distinct
/// logical/source-unit identities. One unit is not the entire source program.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SourceUnit {
    /// Logical module root, resolved through the retained catalog definition table.
    pub module: ModuleHandle,
    /// Source-database identity for this logical source unit.
    pub file: FileId,
    /// Revision of the source used for this lowering.
    pub revision: Revision,
    /// Identity of the node storage whose local IDs this unit qualifies.
    pub arena: HirArenaId,
}

impl SourceUnit {
    /// Captures module/file/revision/arena from one matching lowering.
    pub(crate) fn of(lowered: &LoweredModule, definitions: &DefinitionTable) -> Self {
        Self {
            module: definitions
                .lookup_module(lowered.source.module_identity())
                .expect("registered source module"),
            file: lowered.source.id(),
            revision: lowered.source.revision(),
            arena: lowered.module.body.arena(),
        }
    }

    /// Checks the complete identity before local IDs may index the supplied lowering.
    pub(crate) fn matches(&self, lowered: &LoweredModule, definitions: &DefinitionTable) -> bool {
        definitions.lookup_module(lowered.source.module_identity()) == Some(self.module)
            && self.file == lowered.source.id()
            && self.revision == lowered.source.revision()
            && self.arena == lowered.module.body.arena()
    }
}

/// A declaration handle inside a qualified source unit; excludes imports and module headers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceItem {
    /// A local `TypeAliasId` interpreted in the enclosing `SourceDeclRef.unit`.
    TypeAlias(TypeAliasId),
    /// A local `FunctionId` interpreted in the enclosing `SourceDeclRef.unit`.
    Function(FunctionId),
    /// A local `ConstId` interpreted in the enclosing `SourceDeclRef.unit`.
    Const(ConstId),
    /// A local `StaticId` interpreted in the enclosing `SourceDeclRef.unit`.
    Static(StaticId),
    /// A local `StructId` interpreted in the enclosing `SourceDeclRef.unit`.
    Struct(StructId),
    /// A local `EnumId` interpreted in the enclosing `SourceDeclRef.unit`.
    Enum(EnumId),
    /// A local `OpaqueTypeId` interpreted in the enclosing `SourceDeclRef.unit`.
    OpaqueType(OpaqueTypeId),
    /// A local `TraitId` interpreted in the enclosing `SourceDeclRef.unit`.
    Trait(TraitId),
    /// A local `VariantId` interpreted in the enclosing `SourceDeclRef.unit`.
    Variant(VariantId),
}

impl SourceItem {
    pub(crate) fn namespace(self) -> NameNamespace {
        match self {
            Self::Function(_) | Self::Const(_) | Self::Static(_) | Self::Variant(_) => {
                NameNamespace::Value
            }
            Self::TypeAlias(_)
            | Self::Struct(_)
            | Self::Enum(_)
            | Self::OpaqueType(_)
            | Self::Trait(_) => NameNamespace::Type,
        }
    }

    /// Converts eligible declaration kinds to local names; variants retain qualified source references.
    pub(crate) fn local(self) -> Option<ResolvedName> {
        Some(match self {
            Self::Function(id) => ResolvedName::Function(id),
            Self::Const(id) => ResolvedName::Const(id),
            Self::Static(id) => ResolvedName::Static(id),
            Self::TypeAlias(id) => ResolvedName::TypeAlias(id),
            Self::Struct(id) => ResolvedName::Struct(id),
            Self::Enum(id) => ResolvedName::Enum(id),
            Self::OpaqueType(id) => ResolvedName::OpaqueType(id),
            Self::Trait(id) => ResolvedName::Trait(id),
            Self::Variant(_) => return None,
        })
    }
}

/// A cross-module declaration address, independent of the spelling used to import it.
///
/// ```text
/// use pkg::math::sum as add;
/// app NameTable["add"] -> candidate.target = Source(SourceDeclRef)
/// SourceDeclRef
/// +-- unit: { module: pkg::math, file, revision, arena }
/// `-- item: Function(f)
///
/// lookup source unit -> check it matches retained LoweredModule
///                    -> math.module.functions[f.index()]
/// ```
///
/// Aliases and re-exports retain this same target. The local function ID must never
/// index `app`'s function vector. [`crate::program::CheckedProgram::source_function`]
/// is a checked-program consumer; signature projection also verifies the source unit.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SourceDeclRef {
    /// Complete identity of the lowering that owns the declaration.
    pub unit: SourceUnit,
    /// Local declaration handle interpreted only in `unit`.
    pub item: SourceItem,
}

impl SourceDeclRef {
    /// Returns the local function handle for a function target, otherwise `None`.
    pub fn function(&self) -> Option<FunctionId> {
        match self.item {
            SourceItem::Function(id) => Some(id),
            _ => None,
        }
    }
}

/// Identity of a container whose members are looked up in the shared namespace catalog.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NamespaceId {
    /// A source module's scope, qualified by its complete source unit.
    Module(SourceUnit),
    /// Associated members of a canonical source declaration, such as enum variants.
    Associated(SourceDeclRef),
    /// A package/module prefix synthesized from installed inputs, without a source body.
    InstalledPrefix(ModuleHandle),
}

/// Canonical lookup destination, without aliases, provenance or embedded member tables.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ResolvedTarget {
    /// A namespace that lookup can enter for the next path component.
    Namespace(NamespaceId),
    /// A declaration owned by a qualified source unit.
    Source(SourceDeclRef),
}

impl ResolvedTarget {
    /// The binding category of this canonical declaration or namespace.
    pub fn namespace(&self) -> NameNamespace {
        match self {
            Self::Namespace(_) => NameNamespace::Type,
            Self::Source(source) => source.item.namespace(),
        }
    }

    /// Converts to a resolver name; localizes source IDs only when the full supplied unit matches.
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
        }
    }
}

/// Identity of a real flattened use leaf in its owning source unit.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DirectiveId {
    /// Lowering that contains the original use syntax.
    pub unit: SourceUnit,
    /// Zero-based slot in the lowering's `Module.imports` vector.
    pub slot: u32,
}

/// A single validated binding name, never a `::`-qualified path.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LocalName(String);

impl LocalName {
    /// Accepts a nonempty identifier-like spelling; rejects path separators and invalid characters.
    pub(crate) fn new(name: &str) -> Option<Self> {
        let mut chars = name.chars();
        let first = chars.next()?;
        ((first == '_' || first.is_alphabetic()) && chars.all(|c| c == '_' || c.is_alphanumeric()))
            .then(|| Self(name.into()))
    }

    /// Borrows the unqualified binding spelling.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Whether a use leaf binds one name or expands a namespace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportKind {
    /// One named target; the default local spelling comes from the path terminal.
    Named {
        /// Only an explicit `as name`; absent for an unrenamed named import.
        alias: Option<LocalName>,
    },
    /// A `*` leaf; eligible members become separate binding candidates.
    Glob,
}

/// One syntax leaf resolves both categories, or one namespace for a glob.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirectiveResolution {
    Named(PerNamespace<LookupOutcome>),
    Glob(LookupOutcome),
}

impl DirectiveResolution {
    /// All canonical targets, including both results of a dual-category import.
    pub fn targets(&self) -> impl Iterator<Item = &ResolvedTarget> {
        let outcomes = match self {
            Self::Named(outcomes) => [Some(&outcomes.types), Some(&outcomes.values)],
            Self::Glob(outcome) => [Some(outcome), None],
        };
        outcomes
            .into_iter()
            .flatten()
            .filter_map(LookupOutcome::target)
    }

    /// A single-result query cannot choose between two different definitions.
    pub fn target(&self) -> Option<&ResolvedTarget> {
        let mut targets = self.targets();
        let first = targets.next()?;
        targets.next().is_none().then_some(first)
    }
}

/// One source use leaf with its resolution, origin sites and direct dependencies.
///
/// The builder recreates these facts during each fixed-point pass. A named directive
/// can create one binding per category; a glob creates multiple candidates. Descending
/// `m::nested::value` does not create directives for `nested` or `value`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportDirective {
    /// Source-unit/leaf identity used by binding provenance.
    pub id: DirectiveId,
    /// Flattened written segments before normalization, excluding aliases/glob stars.
    /// Relative roots and package aliases retain their original spelling.
    pub segments: Vec<String>,
    /// Named leaf with optional explicit alias, or glob.
    pub kind: ImportKind,
    /// Physical source site of this leaf.
    pub span: FileSpan,
    /// Physical root use-tree site shared by its leaves.
    pub root_span: FileSpan,
    /// Visibility assigned to bindings created by this use.
    pub visibility: Visibility,
    /// Last pass's canonical result; publication has no pending directives.
    pub resolution: DirectiveResolution,
    /// Syntactic facade/module edges and resolved-target dependencies, including unresolved path prefixes.
    pub direct_dependencies: BTreeSet<ModuleHandle>,
}

/// Why a candidate exists; distinct from the declaration it ultimately targets.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BindingOrigin {
    /// A declaration introduced directly into its module scope.
    Declaration(SourceDeclRef),
    /// A child-module header in a parent source unit.
    ModuleDeclaration {
        /// Unit containing the header.
        unit: SourceUnit,
        /// Parent-local header slot, not the target namespace identity.
        module: ModuleId,
    },
    /// A real explicit named use leaf.
    NamedImport(DirectiveId),
    /// A real glob use leaf that contributed this member.
    GlobImport(DirectiveId),
    /// An implicit package binding.
    Package(PackageId),
    /// An implicit binding contributed by a prelude namespace.
    Prelude(NamespaceId),
}

/// One possible binding, retaining visibility and provenance even before a target is resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingCandidate {
    /// Canonical destination or an explicit pending/failed binding outcome.
    pub resolution: LookupOutcome,
    /// One acyclic derivation of directive/category keys; origins retain all equal targets.
    pub(crate) support: HashSet<(DirectiveId, NameNamespace)>,
    /// Declaration/import/implicit input that introduced the binding.
    pub origin: BindingOrigin,
    /// Module whose access boundary applies to this binding.
    pub owner: ModuleHandle,
    /// Visibility checked against the lookup importer.
    pub visibility: Visibility,
    /// Physical declaration/use site when available; implicit bindings may have none.
    pub location: Option<FileSpan>,
}

/// All candidates for one local spelling, separated by precedence.
///
/// Selection uses nonempty `strong`, then `globs`, then `implicit`. An unresolved
/// strong import still hides weaker bindings. Multiple strong entries conflict even
/// if they target the same declaration; equal glob targets retain all their origins.
/// Each Type/Value slot owns an independent set of these tiers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NameEntry {
    /// Declarations, module headers and explicit named imports.
    pub strong: Vec<BindingCandidate>,
    /// Candidates contributed by glob imports.
    pub globs: Vec<BindingCandidate>,
    /// Lowest-priority package and prelude bindings.
    pub implicit: Vec<BindingCandidate>,
}

/// Immutable import results for one source unit, shared by declaration/body resolution.
///
/// `scope` is the same `Arc<NameTable>` used by this module's catalog namespace;
/// the catalog owns unfiltered candidates, with visibility evaluated at lookup.
/// Dependencies describe analysis/program relationships, not function-call edges.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModuleImportFacts {
    /// Physical path-prefix sites and selected hits for import navigation.
    pub(crate) path_hits: Vec<(FileSpan, catalog::LookupHit)>,
    /// One record per flattened use leaf, preserving source order.
    pub directives: Vec<ImportDirective>,
    /// Shared module-level binding table, including declarations and implicit bindings.
    pub scope: Arc<NameTable>,
    /// Import/namespace conflicts and unresolved/inaccessible import diagnostics.
    pub diagnostics: Vec<Diagnostic>,
    /// Deduplicated module dependencies used for reachability and cache checks.
    pub dependencies: Vec<ModuleHandle>,
}

/// A logical module graph node pointing to its source revision and import facts.
#[derive(Debug, Clone)]
pub struct ModuleNode {
    /// Source file ID selected for this logical module.
    pub file: FileId,
    /// Source revision used to build its import facts.
    pub revision: Revision,
    /// Shared per-source-unit scope, directives and dependencies.
    pub imports: Arc<ModuleImportFacts>,
}

impl ModuleNode {
    /// Borrows module handles scoped to the graph catalog definition table.
    pub fn dependencies(&self) -> &[ModuleHandle] {
        &self.imports.dependencies
    }
}

/// Snapshot-owned import graph and immutable namespace catalog.
///
/// ```text
/// ModuleGraph
/// +-- nodes: ModuleIdentity -> ModuleNode -> Arc<ModuleImportFacts>
/// +-- source_facts: SourceUnit -> Arc<ModuleImportFacts>
/// `-- catalog: Arc<NamespaceCatalog>
///     `-- namespaces: NamespaceId -> NamespaceTable -> Arc<NameTable>
///                                                    ^ same Arc as facts.scope
/// ```
///
/// The catalog has no back-reference to the graph or lowered modules. Duplicate
/// logical identities retain separate source facts and produce diagnostics; they
/// are not silently merged. The graph is an analysis input set, not a Rust crate.
#[derive(Debug, Clone, Default)]
pub struct ModuleGraph {
    /// One representative node per logical identity, with duplicates diagnosed.
    nodes: BTreeMap<ModuleHandle, ModuleNode>,
    order: Vec<ModuleHandle>,
    /// Facts retained separately for every exact source unit.
    source_facts: HashMap<SourceUnit, Arc<ModuleImportFacts>>,
    /// Shared canonical namespace tables used by all lookup consumers.
    pub catalog: Arc<catalog::NamespaceCatalog>,
    #[cfg(test)]
    pub(crate) work: solver::SolverWork,
}

/// Failure to collect an admissible reachable module set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleOrderError {
    /// The root or a dependency has no graph node.
    Missing(ModuleIdentity),
    /// The module handle does not belong to the retained graph table.
    Identity(DefinitionTableError),
    /// A reachable module has import diagnostics.
    InvalidImports(ModuleIdentity),
    /// Cooperative cancellation interrupted traversal.
    Cancelled,
}
