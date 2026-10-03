use crate::hir::{
    expr::ExprData,
    ids::{BlockId, ExprId, HirArenaId, HirOwner, PatternId, PlaceId, StmtId, TypeRefId},
    pattern::PatternData,
    place::PlaceData,
    stmt::{BlockData, StmtData},
    ty::TypeData,
};

#[derive(Debug, Clone, Default)]
pub struct Body {
    pub(crate) arena: HirArenaId,
    pub(crate) blocks: Vec<(HirOwner, BlockData)>,
    pub(crate) stmts: Vec<(HirOwner, StmtData)>,
    pub(crate) exprs: Vec<(HirOwner, ExprData)>,
    pub(crate) places: Vec<(HirOwner, PlaceData)>,
    pub(crate) patterns: Vec<(HirOwner, PatternData)>,
    pub(crate) types: Vec<(HirOwner, TypeData)>,
}

impl Body {
    pub fn arena(&self) -> HirArenaId {
        self.arena
    }

    pub fn expressions(&self) -> impl Iterator<Item = (ExprId, &ExprData)> {
        self.exprs
            .iter()
            .enumerate()
            .map(|(index, (owner, expr))| (ExprId::new(self.arena, *owner, index), expr))
    }

    pub fn places(&self) -> impl Iterator<Item = (PlaceId, &PlaceData)> {
        self.places
            .iter()
            .enumerate()
            .map(|(index, (owner, place))| (PlaceId::new(self.arena, *owner, index), place))
    }

    pub fn statements(&self) -> impl Iterator<Item = (StmtId, &StmtData)> {
        self.stmts
            .iter()
            .enumerate()
            .map(|(index, (owner, stmt))| (StmtId::new(self.arena, *owner, index), stmt))
    }

    pub fn blocks(&self) -> impl Iterator<Item = (BlockId, &BlockData)> {
        self.blocks
            .iter()
            .enumerate()
            .map(|(index, (owner, block))| (BlockId::new(self.arena, *owner, index), block))
    }

    pub fn block(&self, id: BlockId) -> &BlockData {
        assert_eq!(id.arena(), self.arena, "foreign HIR block");
        let (owner, node) = &self.blocks[id.index()];
        assert_eq!(id.owner(), *owner, "foreign HIR body node");
        node
    }

    pub fn stmt(&self, id: StmtId) -> &StmtData {
        assert_eq!(id.arena(), self.arena, "foreign HIR statement");
        let (owner, node) = &self.stmts[id.index()];
        assert_eq!(id.owner(), *owner, "foreign HIR body node");
        node
    }

    pub fn expr(&self, id: ExprId) -> &ExprData {
        assert_eq!(id.arena(), self.arena, "foreign HIR expression");
        let (owner, node) = &self.exprs[id.index()];
        assert_eq!(id.owner(), *owner, "foreign HIR body node");
        node
    }

    pub fn place(&self, id: PlaceId) -> &PlaceData {
        assert_eq!(id.arena(), self.arena, "foreign HIR place");
        let (owner, node) = &self.places[id.index()];
        assert_eq!(id.owner(), *owner, "foreign HIR body node");
        node
    }

    pub fn pattern(&self, id: PatternId) -> &PatternData {
        assert_eq!(id.arena(), self.arena, "foreign HIR pattern");
        let (owner, node) = &self.patterns[id.index()];
        assert_eq!(id.owner(), *owner, "foreign HIR body node");
        node
    }

    pub fn type_ref(&self, id: TypeRefId) -> &TypeData {
        assert_eq!(id.arena(), self.arena, "foreign HIR type reference");
        let (owner, node) = &self.types[id.index()];
        assert_eq!(id.owner(), *owner, "foreign HIR body node");
        node
    }
}

pub type StmtDataBuffer = Vec<StmtData>;
pub type PlaceDataBuffer = Vec<PlaceData>;
pub type TypeDataBuffer = Vec<TypeData>;
