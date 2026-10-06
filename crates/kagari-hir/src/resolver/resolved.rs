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

#[derive(Debug, Clone)]
pub struct ScopeBinding {
    pub name: String,
    pub resolved: ResolvedName,
    pub visible_from: usize,
}

#[derive(Debug, Clone)]
pub struct LexicalScope {
    pub owner: BodyOwner,
    pub span: Span,
    pub parent: Option<usize>,
    pub bindings: Vec<ScopeBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ResolvedName {
    OpaqueType(OpaqueTypeId),
    Source(SourceDeclRef),
    Namespace(NamespaceId),
    HostModule(HostModuleId),
    HostFunction(HostFunctionId),
    HostType(HostTypeId),
    Function(FunctionId),
    Const(ConstId),
    Param(ParamId),
    Local(LocalId),
    Module(ModuleId),
    RuntimeHelper(BuiltinFunction),
    Struct(StructId),
    Enum(EnumId),
    Trait(TraitId),
}

#[derive(Debug, Clone)]
pub struct DeclarationNames {
    pub imports: Arc<ModuleImportFacts>,
    pub catalog: Arc<NamespaceCatalog>,
    pub hosts: Arc<HostDeclarations>,
    pub items: Arc<NameTable>,
}

#[derive(Debug, Clone)]
pub struct QualifiedMember {
    pub owner: ResolvedName,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct ResolvedNames {
    pub imports: Arc<ModuleImportFacts>,
    pub catalog: Arc<NamespaceCatalog>,
    pub hosts: Arc<HostDeclarations>,
    pub items: Arc<NameTable>,
    pub(crate) scopes: Vec<LexicalScope>,
    exprs: HashMap<ExprId, ResolvedName>,
    pub(crate) lookup_hits: HashMap<ExprId, LookupHit>,
    places: HashMap<PlaceId, ResolvedName>,
    qualified_members: HashMap<ExprId, QualifiedMember>,
    pub(crate) pattern_variants: HashMap<PatternId, ResolvedName>,
    closure_captures: HashMap<ExprId, Vec<ResolvedName>>,
}

impl ResolvedNames {
    pub fn lookup_hit(&self, id: ExprId) -> Option<&LookupHit> {
        self.lookup_hits.get(&id)
    }

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

    pub fn qualified_member(&self, id: ExprId) -> Option<&QualifiedMember> {
        self.qualified_members.get(&id)
    }

    pub(crate) fn insert_place(&mut self, id: PlaceId, resolved: ResolvedName) {
        self.places.insert(id, resolved);
    }

    pub fn expr_resolution(&self, id: ExprId) -> Option<ResolvedName> {
        self.exprs.get(&id).cloned()
    }

    pub fn place_resolution(&self, id: PlaceId) -> Option<ResolvedName> {
        self.places.get(&id).cloned()
    }

    pub fn closure_captures(&self, id: ExprId) -> &[ResolvedName] {
        self.closure_captures.get(&id).map_or(&[], Vec::as_slice)
    }

    pub(crate) fn insert_closure_captures(&mut self, id: ExprId, captures: Vec<ResolvedName>) {
        self.closure_captures.insert(id, captures);
    }

    pub fn scopes(&self) -> &[LexicalScope] {
        &self.scopes
    }

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
