use smallvec::{SmallVec, smallvec};

use crate::hir::{
    BlockData, BlockId, ExprData, ExprId, PatternData, PatternId, PlaceData, PlaceId, StmtData,
    StmtId, TypeData, TypeRefId,
};

#[derive(Debug, Clone)]
pub struct Body {
    pub(crate) blocks: BlockDataBuffer,
    pub(crate) stmts: StmtDataBuffer,
    pub(crate) exprs: ExprDataBuffer,
    pub(crate) places: PlaceDataBuffer,
    pub(crate) patterns: PatternDataBuffer,
    pub(crate) types: TypeDataBuffer,
}

impl Default for Body {
    fn default() -> Self {
        Self {
            blocks: smallvec![],
            stmts: smallvec![],
            exprs: smallvec![],
            places: smallvec![],
            patterns: smallvec![],
            types: smallvec![],
        }
    }
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

pub type BlockDataBuffer = SmallVec<[BlockData; 16]>;
pub type StmtDataBuffer = SmallVec<[StmtData; 32]>;
pub type ExprDataBuffer = SmallVec<[ExprData; 64]>;
pub type PlaceDataBuffer = SmallVec<[PlaceData; 16]>;
pub type PatternDataBuffer = SmallVec<[PatternData; 16]>;
pub type TypeDataBuffer = SmallVec<[TypeData; 16]>;
