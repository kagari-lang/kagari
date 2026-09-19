use crate::hir::{
    BlockData, BlockId, ExprData, ExprId, PatternData, PatternId, PlaceData, PlaceId, StmtData,
    StmtId, TypeData, TypeRefId,
};

#[derive(Debug, Clone, Default)]
pub struct Body {
    pub(crate) arena: crate::hir::HirArenaId,
    pub(crate) blocks: BlockDataBuffer,
    pub(crate) stmts: StmtDataBuffer,
    pub(crate) exprs: ExprDataBuffer,
    pub(crate) places: PlaceDataBuffer,
    pub(crate) patterns: PatternDataBuffer,
    pub(crate) types: TypeDataBuffer,
}

impl Body {
    pub fn arena(&self) -> crate::hir::HirArenaId {
        self.arena
    }
    pub fn expressions(&self) -> impl Iterator<Item = (ExprId, &ExprData)> {
        self.exprs
            .iter()
            .enumerate()
            .map(|(index, expr)| (ExprId::new(self.arena, index), expr))
    }

    pub fn blocks(&self) -> impl Iterator<Item = (BlockId, &BlockData)> {
        self.blocks
            .iter()
            .enumerate()
            .map(|(index, block)| (BlockId::new(self.arena, index), block))
    }
    pub fn block(&self, id: BlockId) -> &BlockData {
        assert_eq!(id.arena(), self.arena, "foreign HIR block");
        &self.blocks[id.index()]
    }

    pub fn stmt(&self, id: StmtId) -> &StmtData {
        assert_eq!(id.arena(), self.arena, "foreign HIR statement");
        &self.stmts[id.index()]
    }

    pub fn expr(&self, id: ExprId) -> &ExprData {
        assert_eq!(id.arena(), self.arena, "foreign HIR expression");
        &self.exprs[id.index()]
    }

    pub fn place(&self, id: PlaceId) -> &PlaceData {
        assert_eq!(id.arena(), self.arena, "foreign HIR place");
        &self.places[id.index()]
    }

    pub fn pattern(&self, id: PatternId) -> &PatternData {
        assert_eq!(id.arena(), self.arena, "foreign HIR pattern");
        &self.patterns[id.index()]
    }

    pub fn type_ref(&self, id: TypeRefId) -> &TypeData {
        assert_eq!(id.arena(), self.arena, "foreign HIR type reference");
        &self.types[id.index()]
    }
}

pub type BlockDataBuffer = Vec<BlockData>;
pub type StmtDataBuffer = Vec<StmtData>;
pub type ExprDataBuffer = Vec<ExprData>;
pub type PlaceDataBuffer = Vec<PlaceData>;
pub type PatternDataBuffer = Vec<PatternData>;
pub type TypeDataBuffer = Vec<TypeData>;
