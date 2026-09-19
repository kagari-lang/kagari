macro_rules! id_newtype {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub struct $name(u32);

        impl $name {
            pub fn new(index: usize) -> Self {
                Self(index as u32)
            }

            pub fn index(self) -> usize {
                self.0 as usize
            }
        }
    };
}

/// Identity of one immutable lowering, shared only when that lowering is reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HirArenaId(u64);

impl Default for HirArenaId {
    fn default() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT_ARENA: AtomicU64 = AtomicU64::new(1);
        Self(
            NEXT_ARENA
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                .expect("HIR arena identity exhausted"),
        )
    }
}

macro_rules! local_id_newtype {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub struct $name {
            arena: HirArenaId,
            index: u32,
        }
        impl $name {
            pub(crate) fn new(arena: HirArenaId, index: usize) -> Self {
                Self {
                    arena,
                    index: u32::try_from(index).expect("HIR arena capacity exhausted"),
                }
            }
            pub fn arena(self) -> HirArenaId {
                self.arena
            }
            pub fn index(self) -> usize {
                self.index as usize
            }
        }
    };
}

id_newtype!(FunctionId);
id_newtype!(MethodId);
id_newtype!(ConstId);
id_newtype!(TraitId);
id_newtype!(TraitMethodId);
id_newtype!(ImplId);
id_newtype!(ModuleId);
local_id_newtype!(ParamId);
local_id_newtype!(LocalId);
id_newtype!(StructId);
id_newtype!(EnumId);
local_id_newtype!(BlockId);
local_id_newtype!(ExprId);
local_id_newtype!(PlaceId);
local_id_newtype!(StmtId);
local_id_newtype!(PatternId);
local_id_newtype!(TypeRefId);
id_newtype!(GenericParamId);

#[derive(Debug, Clone, Copy)]
pub(crate) enum BodySelection {
    All,
    Function(FunctionId),
}

impl BodySelection {
    pub(crate) fn includes(self, id: FunctionId) -> bool {
        matches!(self, Self::All) || matches!(self, Self::Function(selected) if selected == id)
    }
}

/// A field slot within its declaring struct in this analysis's HIR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FieldId {
    pub owner: StructId,
    pub slot: usize,
}
