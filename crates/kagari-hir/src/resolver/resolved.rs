use std::collections::HashMap;

use crate::builtin::surface;
use crate::hir::{
    ConstId, EnumId, ExprId, FunctionId, LocalId, ModuleId, ParamId, PlaceId, StructId, TraitId,
};
use crate::resolver::table::NameTable;
use kagari_common::Span;

use crate::hir::BodyOwner;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResolvedName {
    SourceImport(usize),
    SourceItem {
        import: usize,
        item: crate::hir::ExportItem,
    },
    HostModule(crate::host::HostModuleId),
    HostFunction(crate::host::HostFunctionId),
    HostType(crate::host::HostTypeId),
    Function(FunctionId),
    Const(ConstId),
    Param(ParamId),
    Local(LocalId),
    Module(ModuleId),
    StandardModule(surface::StandardModule),
    StandardFunction(surface::StandardIntrinsic),
    RuntimeHelper(crate::builtin::BuiltinFunction),
    Struct(StructId),
    Enum(EnumId),
    Trait(TraitId),
}

#[derive(Debug, Clone)]
pub struct DeclarationNames {
    pub imports: std::sync::Arc<crate::imports::ModuleImports>,
    pub hosts: std::sync::Arc<crate::host::HostDeclarations>,
    pub items: std::sync::Arc<NameTable>,
}

#[derive(Debug, Clone)]
pub struct QualifiedMember {
    pub owner: ResolvedName,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct ResolvedNames {
    pub imports: std::sync::Arc<crate::imports::ModuleImports>,
    pub hosts: std::sync::Arc<crate::host::HostDeclarations>,
    pub items: std::sync::Arc<NameTable>,
    pub(crate) scopes: Vec<LexicalScope>,
    exprs: HashMap<ExprId, ResolvedName>,
    places: HashMap<PlaceId, ResolvedName>,
    qualified_members: HashMap<ExprId, QualifiedMember>,
    closure_captures: HashMap<ExprId, Vec<ResolvedName>>,
}

impl ResolvedNames {
    pub(crate) fn new(
        items: std::sync::Arc<NameTable>,
        hosts: std::sync::Arc<crate::host::HostDeclarations>,
        imports: std::sync::Arc<crate::imports::ModuleImports>,
    ) -> Self {
        Self {
            imports,
            hosts,
            items,
            scopes: Vec::new(),
            exprs: HashMap::new(),
            places: HashMap::new(),
            qualified_members: HashMap::new(),
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
        self.exprs.get(&id).copied()
    }

    pub fn place_resolution(&self, id: PlaceId) -> Option<ResolvedName> {
        self.places.get(&id).copied()
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
            .min_by_key(|(id, scope)| (scope.span.end - scope.span.start, std::cmp::Reverse(*id)))
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
