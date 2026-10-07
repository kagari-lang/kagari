//! Lexical scope history and node-keyed binding facts, separate from checked type/call facts.

use crate::{
    builtin::BuiltinFunction,
    hir::{
        ids::{
            BodyOwner, ConstId, EnumId, ExprId, FunctionId, LocalId, ModuleId, OpaqueTypeId,
            ParamId, PatternId, PlaceId, StructId, TraitId,
        },
        item::Module,
        pattern::PatternKind,
    },
    host::{HostDeclarations, HostFunctionId, HostModuleId, HostTypeId},
    imports::{
        ModuleImportFacts, NamespaceId, SourceDeclRef,
        catalog::{LookupHit, NamespaceCatalog},
    },
    resolver::table::NameTable,
};
use std::{cmp::Reverse, collections::HashMap, sync::Arc};

use kagari_common::span::Span;

/// A local/parameter binding together with when it becomes visible in source.
#[derive(Debug, Clone)]
pub struct ScopeBinding {
    /// Unqualified local spelling.
    pub name: String,
    /// Parameter/local identity selected by lexical resolution.
    pub resolved: ResolvedName,
    /// Inclusive byte offset from which the binding is visible within its scope.
    pub visible_from: usize,
}

/// A body-owned lexical region and its source-ordered bindings.
#[derive(Debug, Clone)]
pub struct LexicalScope {
    /// Function or constant to which this region belongs.
    pub owner: BodyOwner,
    /// Half-open byte range used for position-based scope queries.
    pub span: Span,
    /// Index of the parent in `ResolvedNames.scopes`, or absent at a root scope.
    pub parent: Option<usize>,
    /// Binding history, retaining shadowed entries and visibility start offsets.
    pub bindings: Vec<ScopeBinding>,
}

/// A bound local, qualified declaration, namespace or installed/builtin target.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ResolvedName {
    /// A local native-backed type declaration.
    OpaqueType(OpaqueTypeId),
    /// A canonical declaration in a qualified source unit.
    Source(SourceDeclRef),
    /// A member container identified through the namespace catalog.
    Namespace(NamespaceId),
    /// An installed host module.
    HostModule(HostModuleId),
    /// An installed host function.
    HostFunction(HostFunctionId),
    /// An installed host type.
    HostType(HostTypeId),
    /// A local function slot.
    Function(FunctionId),
    /// A local constant slot.
    Const(ConstId),
    /// A parameter in the current body's function.
    Param(ParamId),
    /// A local binding in the current body.
    Local(LocalId),
    /// A parent-local child module header.
    Module(ModuleId),
    /// A recognized compiler/runtime helper surface.
    RuntimeHelper(BuiltinFunction),
    /// A local struct declaration.
    Struct(StructId),
    /// A local enum declaration.
    Enum(EnumId),
    /// A local trait declaration.
    Trait(TraitId),
}

/// Shared module-level lookup inputs prepared before traversing bodies.
#[derive(Debug, Clone)]
pub struct DeclarationNames {
    /// Shared directives, dependencies and scope facts for this source unit.
    pub imports: Arc<ModuleImportFacts>,
    /// Shared immutable namespace catalog for qualified lookup.
    pub catalog: Arc<NamespaceCatalog>,
    /// Shared installed host declaration universe.
    pub hosts: Arc<HostDeclarations>,
    /// The module's shared name table; separate from body lexical scopes.
    pub items: Arc<NameTable>,
}

/// A resolved owner plus member spelling that requires semantic member selection.
#[derive(Debug, Clone)]
pub struct QualifiedMember {
    /// Resolved declaration/type owner, before selecting the member.
    pub owner: ResolvedName,
    /// Member spelling, checked against the owner later.
    pub name: String,
}

/// Body resolution facts keyed by the matching lowering's node IDs.
///
/// The internal `resolve_bodies` pass fills these maps and lexical scopes.
/// The module table/catalog are shared; local declarations do not mutate module
/// bindings. For `val y = x + 1; y`, the first name resolves to a parameter and the
/// tail name to the binding's `LocalId`. `y` becomes visible after its initializer.
///
/// | Storage | Key -> value | Absence |
/// | --- | --- | --- |
/// | `exprs` / `places` | expression/place ID -> `ResolvedName` | No recorded binding; literals and unresolved names need not have entries. |
/// | `qualified_members` | expression ID -> owner + member spelling | Not a deferred qualified member. |
/// | `lookup_hits` / `path_hits` | expression ID / byte site -> target + provenance | No successful namespace hit at that site. |
/// | `pattern_variants` | pattern ID -> selected variant | A bare name may instead be a binding. |
/// | `closure_captures` | closure expression ID -> captured bindings | Getter returns an empty slice. |
///
/// Resolution does not select a checked call signature. Call targets, substitutions
/// and expression types belong to [`crate::typeck::table::TypeTable`].
#[derive(Debug, Clone)]
pub struct ResolvedNames {
    /// Shared directives, dependencies and scope facts for this source unit.
    pub imports: Arc<ModuleImportFacts>,
    /// Shared immutable namespace catalog for qualified lookup.
    pub catalog: Arc<NamespaceCatalog>,
    /// Shared installed host declaration universe.
    pub hosts: Arc<HostDeclarations>,
    /// The module's shared name table; separate from body lexical scopes.
    pub items: Arc<NameTable>,
    pub(crate) scopes: Vec<LexicalScope>,
    exprs: HashMap<ExprId, ResolvedName>,
    pub(crate) lookup_hits: HashMap<ExprId, LookupHit>,
    pub(crate) path_hits: Vec<(Span, LookupHit)>,
    places: HashMap<PlaceId, ResolvedName>,
    qualified_members: HashMap<ExprId, QualifiedMember>,
    pub(crate) pattern_variants: HashMap<PatternId, ResolvedName>,
    closure_captures: HashMap<ExprId, Vec<ResolvedName>>,
}

impl ResolvedNames {
    /// Borrows a recorded namespace hit with provenance, if resolution produced one.
    pub fn lookup_hit(&self, id: ExprId) -> Option<&LookupHit> {
        self.lookup_hits.get(&id)
    }

    /// Classifies pattern structure using resolved bare-name variants; requires the matching module and is not full type validation.
    pub fn pattern_is_irrefutable(&self, module: &Module, id: PatternId) -> bool {
        match &module.pattern(id).kind {
            PatternKind::Wildcard => true,
            PatternKind::Name { .. } => !self.pattern_variants.contains_key(&id),
            PatternKind::Tuple(elements) => elements
                .iter()
                .all(|element| self.pattern_is_irrefutable(module, *element)),
            PatternKind::Struct { fields, .. } => fields
                .iter()
                .all(|field| self.pattern_is_irrefutable(module, field.pattern)),
            PatternKind::Or(alternatives) => alternatives
                .iter()
                .any(|alternative| self.pattern_is_irrefutable(module, *alternative)),
            PatternKind::Range { .. }
            | PatternKind::Literal(_)
            | PatternKind::EnumVariant { .. } => false,
        }
    }

    pub(crate) fn new(
        items: Arc<NameTable>,
        hosts: Arc<HostDeclarations>,
        imports: Arc<ModuleImportFacts>,
        catalog: Arc<NamespaceCatalog>,
    ) -> Self {
        Self {
            imports,
            catalog,
            hosts,
            items,
            scopes: Vec::new(),
            exprs: HashMap::new(),
            lookup_hits: HashMap::new(),
            path_hits: Vec::new(),
            places: HashMap::new(),
            qualified_members: HashMap::new(),
            pattern_variants: HashMap::new(),
            closure_captures: HashMap::new(),
        }
    }

    pub(crate) fn insert_expr(&mut self, id: ExprId, resolved: ResolvedName) {
        self.exprs.insert(id, resolved);
    }

    pub(crate) fn insert_qualified_member(&mut self, id: ExprId, member: QualifiedMember) {
        self.qualified_members.insert(id, member);
    }

    /// Borrows a deferred qualified-member selection, or `None` for other expressions.
    pub fn qualified_member(&self, id: ExprId) -> Option<&QualifiedMember> {
        self.qualified_members.get(&id)
    }

    pub(crate) fn insert_place(&mut self, id: PlaceId, resolved: ResolvedName) {
        self.places.insert(id, resolved);
    }

    /// Clones the recorded expression binding, or `None` when no binding was recorded.
    pub fn expr_resolution(&self, id: ExprId) -> Option<ResolvedName> {
        self.exprs.get(&id).cloned()
    }

    /// Clones the recorded place-root binding, or `None` when no binding was recorded.
    pub fn place_resolution(&self, id: PlaceId) -> Option<ResolvedName> {
        self.places.get(&id).cloned()
    }

    /// Borrows captured bindings in their recorded order; absent entries return an empty slice.
    pub fn closure_captures(&self, id: ExprId) -> &[ResolvedName] {
        self.closure_captures.get(&id).map_or(&[], Vec::as_slice)
    }

    pub(crate) fn insert_closure_captures(&mut self, id: ExprId, captures: Vec<ResolvedName>) {
        self.closure_captures.insert(id, captures);
    }

    /// Borrows all recorded lexical scopes; parent indices refer to this slice.
    pub fn scopes(&self) -> &[LexicalScope] {
        &self.scopes
    }

    /// Returns visible lexical bindings at a byte offset, sorted by name.
    ///
    /// Selects the smallest containing scope (latest entry breaks equal-range ties),
    /// then walks parents. Later visible bindings shadow earlier/outer spellings.
    /// Module-level declarations are not included; offsets outside all scopes yield none.
    pub fn visible_bindings(&self, offset: usize) -> Vec<&ScopeBinding> {
        let mut scope = self
            .scopes
            .iter()
            .enumerate()
            .filter(|(_, scope)| scope.span.start <= offset && offset < scope.span.end)
            .min_by_key(|(id, scope)| (scope.span.end - scope.span.start, Reverse(*id)))
            .map(|(id, _)| id);
        let mut visible = HashMap::new();
        while let Some(id) = scope {
            for binding in self.scopes[id].bindings.iter().rev() {
                if binding.visible_from <= offset {
                    visible.entry(binding.name.as_str()).or_insert(binding);
                }
            }
            scope = self.scopes[id].parent;
        }
        let mut visible = visible.into_values().collect::<Vec<_>>();
        visible.sort_by(|a, b| a.name.cmp(&b.name));
        visible
    }
}
