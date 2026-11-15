//! Canonical lexical bindings and node-keyed facts, separate from checked type/call facts.

use crate::{
    builtin::BuiltinFunction,
    hir::{
        body::Body,
        ids::{
            BodyOwner, ConstId, EnumId, ExprId, FunctionId, LocalId, ModuleId, OpaqueTypeId,
            ParamId, PatternId, PlaceId, StaticId, StructId, TraitId, TypeAliasId,
        },
        pattern::PatternKind,
        writeability::Writeability,
    },
    imports::{ModuleImportFacts, NamespaceId, SourceDeclRef, catalog::NamespaceCatalog},
    resolver::table::NameTable,
};
use std::{collections::HashMap, sync::Arc};

/// One canonical lexical declaration in the matching lowering, including unused binders.
#[derive(Debug, Clone)]
pub struct Binding {
    /// Unqualified local spelling.
    pub name: String,
    /// Parameter, local or named-local-function identity selected by lexical resolution.
    pub resolved: ResolvedName,
    /// Semantic body containing the declaration (not the body of a named local function).
    pub owner: BodyOwner,
    /// Declared rebinding policy; function names and pattern/closure parameters are immutable.
    pub writeability: Writeability,
}

/// A bound local, qualified declaration, namespace or installed/builtin target.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ResolvedName {
    /// A local transparent type alias declaration.
    TypeAlias(TypeAliasId),
    /// A local native-backed type declaration.
    OpaqueType(OpaqueTypeId),
    /// A canonical declaration in a qualified source unit.
    Source(SourceDeclRef),
    /// A member container identified through the namespace catalog.
    Namespace(NamespaceId),
    /// A local function slot.
    Function(FunctionId),
    /// A local constant slot.
    Const(ConstId),
    /// A module-owned runtime static slot; never a closure capture.
    Static(StaticId),
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
/// The internal `resolve_bodies` pass fills these maps and the binding directory.
/// The module table/catalog are shared; local declarations do not mutate module
/// bindings. For `val y = x + 1; y`, the first name resolves to a parameter and the
/// tail name to the binding's `LocalId`. `y` becomes visible after its initializer.
///
/// | Storage | Key -> value | Absence |
/// | --- | --- | --- |
/// | `exprs` / `places` | expression/place ID -> `ResolvedName` | No recorded binding; literals and unresolved names need not have entries. |
/// | `qualified_members` | expression ID -> owner + member spelling | Not a deferred qualified member. |
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
    /// The module's shared name table; separate from body lexical scopes.
    pub items: Arc<NameTable>,
    /// Qualified lexical identity -> authoritative binding metadata; no cursor history.
    bindings: HashMap<ResolvedName, Binding>,
    exprs: HashMap<ExprId, ResolvedName>,
    places: HashMap<PlaceId, ResolvedName>,
    qualified_members: HashMap<ExprId, QualifiedMember>,
    pub(crate) pattern_variants: HashMap<PatternId, ResolvedName>,
    closure_captures: HashMap<ExprId, Vec<ResolvedName>>,
}

impl ResolvedNames {
    /// Classifies pattern structure using resolved bare-name variants; requires the matching body arena and is not full type validation.
    pub fn pattern_is_irrefutable(&self, body: &Body, id: PatternId) -> bool {
        match &body.pattern(id).kind {
            PatternKind::Wildcard => true,
            PatternKind::Name { .. } => !self.pattern_variants.contains_key(&id),
            PatternKind::Tuple(elements) => elements
                .iter()
                .all(|element| self.pattern_is_irrefutable(body, *element)),
            PatternKind::Struct { fields, .. } => fields
                .iter()
                .all(|field| self.pattern_is_irrefutable(body, field.pattern)),
            PatternKind::Or(alternatives) => alternatives
                .iter()
                .any(|alternative| self.pattern_is_irrefutable(body, *alternative)),
            PatternKind::Range { .. }
            | PatternKind::Literal(_)
            | PatternKind::EnumVariant { .. } => false,
        }
    }

    pub(crate) fn new(
        items: Arc<NameTable>,
        imports: Arc<ModuleImportFacts>,
        catalog: Arc<NamespaceCatalog>,
    ) -> Self {
        Self {
            imports,
            catalog,
            items,
            bindings: HashMap::new(),
            exprs: HashMap::new(),
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

    /// Borrows canonical metadata without spelling lookup or a visibility index.
    /// Qualified local/parameter handles reject foreign/stale lowerings. Like other
    /// declaration reads, plain named-function slots require the matching module.
    pub fn binding(&self, resolved: &ResolvedName) -> Option<&Binding> {
        self.bindings.get(resolved)
    }

    /// Enumerates all lexical declarations, including unused and shadowed bindings.
    /// Order is unspecified; named local functions retain their declaring body owner.
    pub fn bindings(&self) -> impl Iterator<Item = &Binding> {
        self.bindings.values()
    }

    pub(crate) fn insert_binding(&mut self, binding: Binding) {
        self.bindings
            .entry(binding.resolved.clone())
            .or_insert(binding);
    }
}
