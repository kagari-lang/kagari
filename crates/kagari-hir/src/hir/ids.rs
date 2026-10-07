//! Local identity families and the storage each family addresses.
//!
//! These handles are not [`kagari_common::identity::table::DefinitionId`] values.
//! Plain IDs contain a `u32`; body-local IDs contain `{ arena, owner, index }`;
//! field/variant IDs contain `{ arena, owner, slot }`.
//!
//! ```text
//! SourceMap::push_function -> FunctionId -> Module.functions[id.index()]
//! SourceMap::push_expr     -> ExprId    -> Body.expr(id) -> exprs[id.index()]
//! SourceMap::push_param    -> ParamId   -> Function.params entry with matching id
//! SourceMap::push_local    -> LocalId   -> binding record / semantic table by id
//! FieldId                 -> Module.field(id)
//!                            -> structs[id.owner().index()].fields[id.slot()]
//! ```
//!
//! Parameter/local indices address allocation-wide source-map slots, not positions
//! within one function's parameter list. A [`HirArenaId`] is allocated by a checked
//! process-wide counter. Fresh lowering receives a fresh arena; cloning/reusing the
//! same lowering retains it. [`HirOwner`] partitions records logically; it does not
//! make their vector indices relative to the owner. It is distinct from an enclosing
//! struct/enum owner in a member ID.
//!
//! The internal `Lowerer` allocates through the source map and appends
//! matching body rows. Body access checks arena and owner before returning the row.
//! Plain declaration IDs lack those checks and require the matching module.
//! Cross-module references use [`crate::imports::SourceDeclRef`] with its complete
//! source unit; they must not index the importing module's vectors. These are not
//! persistent IDs across reparsing or serialization.

use std::sync::atomic::{AtomicU64, Ordering};

macro_rules! id_newtype {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub struct $name(u32);

        impl $name {
            /// Constructs a local index without checking any declaration collection.
            /// The `usize` is narrowed to `u32`; allocation callers supply bounded indices.
            pub fn new(index: usize) -> Self {
                Self(index as u32)
            }

            /// Returns the zero-based slot, without checking a destination collection.
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
        static NEXT_ARENA: AtomicU64 = AtomicU64::new(1);
        Self(
            NEXT_ARENA
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                .expect("HIR arena identity exhausted"),
        )
    }
}

macro_rules! local_id_newtype {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub struct $name {
            arena: HirArenaId,
            owner: HirOwner,
            index: u32,
        }

        impl $name {
            pub(crate) fn new(arena: HirArenaId, owner: HirOwner, index: usize) -> Self {
                Self {
                    arena,
                    owner,
                    index: u32::try_from(index).expect("HIR arena capacity exhausted"),
                }
            }

            /// Returns the identity of the lowering that allocated this handle.
            pub fn arena(self) -> HirArenaId {
                self.arena
            }

            /// Returns the declaration/body owner recorded at allocation.
            pub fn owner(self) -> HirOwner {
                self.owner
            }

            /// Returns the zero-based slot, without checking a destination collection.
            pub fn index(self) -> usize {
                self.index as usize
            }
        }
    };
}

/// A function or constant whose body owns a group of lowered nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BodyOwner {
    /// Nodes belonging to a function, including its parameters and body.
    Function(FunctionId),
    /// Nodes belonging to a constant initializer.
    Const(ConstId),
}

/// The allocation context attached to body-local IDs and their storage rows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum HirOwner {
    /// Declaration syntax outside a function/constant body context.
    #[default]
    Declaration,
    /// Nodes allocated while lowering the indicated function or constant.
    Body(BodyOwner),
}

id_newtype!(
    /// A lowering-local index identifying `Module.functions`.
    /// See the [identity families](crate::hir::ids) for allocation and validity rules.
    FunctionId
);
id_newtype!(
    /// A lowering-local index identifying `Module.methods`.
    /// See the [identity families](crate::hir::ids) for allocation and validity rules.
    MethodId
);
id_newtype!(
    /// A lowering-local index identifying `Module.consts`.
    /// See the [identity families](crate::hir::ids) for allocation and validity rules.
    ConstId
);
id_newtype!(
    /// A lowering-local index identifying `Module.traits`.
    /// See the [identity families](crate::hir::ids) for allocation and validity rules.
    TraitId
);
id_newtype!(
    /// A lowering-local index identifying the source map's trait-method span vector; trait records retain their method IDs.
    /// See the [identity families](crate::hir::ids) for allocation and validity rules.
    TraitMethodId
);
id_newtype!(
    /// A lowering-local index identifying `Module.impls`.
    /// See the [identity families](crate::hir::ids) for allocation and validity rules.
    ImplId
);
id_newtype!(
    /// A lowering-local index identifying `Module.modules` (child headers, not whole-program modules).
    /// See the [identity families](crate::hir::ids) for allocation and validity rules.
    ModuleId
);
local_id_newtype!(
    /// An arena/owner-qualified handle identifying a function parameter record and its source-map slot.
    /// See the [identity families](crate::hir::ids) for allocation and lookup rules.
    ParamId
);
local_id_newtype!(
    /// An arena/owner-qualified handle identifying a statement, pattern or closure binding and its source-map slot.
    /// See the [identity families](crate::hir::ids) for allocation and lookup rules.
    LocalId
);
id_newtype!(
    /// A lowering-local index identifying `Module.structs`.
    /// See the [identity families](crate::hir::ids) for allocation and validity rules.
    StructId
);
id_newtype!(
    /// A lowering-local index identifying `Module.opaque_types`.
    /// See the [identity families](crate::hir::ids) for allocation and validity rules.
    OpaqueTypeId
);
id_newtype!(
    /// A lowering-local index identifying `Module.enums`.
    /// See the [identity families](crate::hir::ids) for allocation and validity rules.
    EnumId
);
local_id_newtype!(
    /// An arena/owner-qualified handle identifying `Body.blocks`.
    /// See the [identity families](crate::hir::ids) for allocation and lookup rules.
    BlockId
);
local_id_newtype!(
    /// An arena/owner-qualified handle identifying `Body.exprs`.
    /// See the [identity families](crate::hir::ids) for allocation and lookup rules.
    ExprId
);
local_id_newtype!(
    /// An arena/owner-qualified handle identifying `Body.places`.
    /// See the [identity families](crate::hir::ids) for allocation and lookup rules.
    PlaceId
);
local_id_newtype!(
    /// An arena/owner-qualified handle identifying `Body.stmts`.
    /// See the [identity families](crate::hir::ids) for allocation and lookup rules.
    StmtId
);
local_id_newtype!(
    /// An arena/owner-qualified handle identifying `Body.patterns`.
    /// See the [identity families](crate::hir::ids) for allocation and lookup rules.
    PatternId
);
local_id_newtype!(
    /// An arena/owner-qualified handle identifying `Body.types` (type syntax, not a resolved semantic type).
    /// See the [identity families](crate::hir::ids) for allocation and lookup rules.
    TypeRefId
);
id_newtype!(
    /// A lowering-local index identifying the source map's generic-parameter spans; declarations retain their parameter records.
    /// See the [identity families](crate::hir::ids) for allocation and validity rules.
    GenericParamId
);

/// Selects all function bodies or one function for incremental body checking.
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

macro_rules! member_id {
    ($(#[$meta:meta])* $name:ident, $owner:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub struct $name {
            arena: HirArenaId,
            owner: $owner,
            slot: u32,
        }

        impl $name {
            pub(crate) fn new(arena: HirArenaId, owner: $owner, slot: usize) -> Self {
                Self {
                    arena,
                    owner,
                    slot: u32::try_from(slot).expect("HIR member capacity exhausted"),
                }
            }

            /// Returns the identity of the lowering that allocated this handle.
            pub fn arena(self) -> HirArenaId {
                self.arena
            }

            /// Returns the enclosing declaration whose member array contains this slot.
            pub fn owner(self) -> $owner {
                self.owner
            }

            /// Returns the zero-based member position within the enclosing declaration.
            pub fn slot(self) -> usize {
                self.slot as usize
            }
        }
    };
}

member_id!(
    /// An arena-qualified struct field slot within its enclosing declaration.
    /// See the [identity families](crate::hir::ids) for member lookup.
    FieldId, StructId
);
member_id!(
    /// An arena-qualified enum variant slot within its enclosing declaration.
    /// See the [identity families](crate::hir::ids) for member lookup.
    VariantId, EnumId
);
