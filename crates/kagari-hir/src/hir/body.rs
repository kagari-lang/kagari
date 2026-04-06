use crate::hir::{
    BlockData, BlockId, ExprData, ExprId, PatternData, PatternId, PlaceData, PlaceId, StmtData,
    StmtId, TypeData, TypeRefId,
};

#[derive(Debug, Clone, Default)]
pub struct Body {
    pub(crate) blocks: BlockDataBuffer,
    pub(crate) stmts: StmtDataBuffer,
    pub(crate) exprs: ExprDataBuffer,
    pub(crate) places: PlaceDataBuffer,
    pub(crate) patterns: PatternDataBuffer,
    pub(crate) types: TypeDataBuffer,
}

impl Body {
    pub fn block(&self, id: BlockId) -> &BlockData {
        &self.blocks[id.index()]
    }

    pub fn stmt(&self, id: StmtId) -> &StmtData {
        &self.stmts[id.index()]
    }

    pub fn expr(&self, id: ExprId) -> &ExprData {
        &self.exprs[id.index()]
    }

    pub fn place(&self, id: PlaceId) -> &PlaceData {
        &self.places[id.index()]
    }

    pub fn pattern(&self, id: PatternId) -> &PatternData {
        &self.patterns[id.index()]
    }

    pub fn type_ref(&self, id: TypeRefId) -> &TypeData {
        &self.types[id.index()]
    }
}

pub type BlockDataBuffer = Vec<BlockData>;
pub type StmtDataBuffer = Vec<StmtData>;
pub type ExprDataBuffer = Vec<ExprData>;
pub type PlaceDataBuffer = Vec<PlaceData>;
pub type PatternDataBuffer = Vec<PatternData>;
pub type TypeDataBuffer = Vec<TypeData>;
