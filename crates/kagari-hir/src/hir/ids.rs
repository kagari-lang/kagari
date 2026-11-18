//! Local identity families and the storage each family addresses.
//!
//! These handles are not [`kagari_common::identity::table::DefinitionId`] values.
//! Plain IDs contain a `u32`; body-local IDs contain `{ arena, owner, index }`;
//! field/variant/trait method IDs contain `{ arena, owner, slot }`.
//!
//! ```text
//! Lowerer::reserve_function -> FunctionId -> Module.functions[id.index()]
//! SourceMap::push_expr     -> ExprId    -> HirNodes.expr(id) -> exprs[id.index()]
//! Lowerer parameter slot   -> ParamId   -> Module.parameter(id) -> Function.params[id.index()]
//! Lowerer::alloc_local    -> LocalId   -> HirNodes.local(id) -> locals[id.index()]
//! FieldId                 -> Module.field(id)
//!                            -> struct/native owner.fields[id.slot()]
//! ```
//!
//! Parameter indices address positions in the owning `Function.params` list;
//! local indices address lowering-wide record slots in `HirNodes.locals`. A [`LoweringId`]
//! is assigned by a checked process-wide counter. Fresh lowering receives a fresh
//! identity; cloning/reusing the same lowering retains it. [`HirOwner`] partitions body
//! records logically; only parameter indices are relative to their function owner.
//! It is distinct from an enclosing struct/enum owner in a member ID.
//!
//! The internal `Lowerer` allocates body nodes through the source map and appends
//! matching body rows. Local records and parameters register spans under their own IDs.
//! Body/parameter access checks lowering identity and owner before returning the record.
//! Plain declaration IDs lack those checks and require the matching module.
//! Cross-module references use [`crate::imports::SourceDeclRef`] with its complete
//! source unit; they must not index the importing module's vectors. These are not
//! persistent IDs across reparsing or serialization.

//! # Reading generated ID fields
//!
//! ```text
//! fn add(x: i32) -> i32 { val y = x + 1; y }
//! id_newtype!(FunctionId)      -> FunctionId(index)
//! local_id_newtype!(ExprId)    -> ExprId { arena: A, owner: Body(Function(f)), index: k }
//! local_id_newtype!(ParamId)   -> ParamId { arena: A, owner: Body(Function(f)), index: p }
//! member_id!(FieldId, FieldOwner) -> FieldId { arena: A, owner: Struct(s), slot: j }
//! ```
//!
//! The macros create separate Rust types: FunctionId and StructId cannot be
//! interchanged even if both contain zero. `index`/`slot` are generated metadata,
//! never identifier spelling. Plain IDs compare only their local indices: two
//! different modules' FunctionId(0) values can compare equal, so callers must carry
//! module context. Qualified IDs compare/hash all fields. The historic `arena` field
//! stores the lowering identity, not an allocator or memory address. `owner` records
//! lowering context, and `index` selects the matching category's storage slot
//! (function-local for parameters, otherwise lowering-wide).
//! A FieldId/VariantId instead uses an enclosing declaration and a member-relative `slot`.
//!
//! ID constructors check the usize-to-u32 conversion.
//! HirNodes/SourceMap accessors for qualified IDs check lowering identity and
//! stored owner on lookup. An ID does not itself borrow or own its target.
//! Rebuilding after inserting a declaration can shift plain IDs; neither macro
//! supplies stable declaration identity across edits. These source-analysis
//! handles are replaced by checked executable identities/operands before execution.

use std::sync::atomic::{AtomicU64, Ordering};

/// Generates a typed u32 index; the caller supplies the matching module/container.
/// See the module examples for FunctionId allocation, lookup and edit validity.
macro_rules! id_newtype {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub struct $name(u32);

        impl $name {
            /// Constructs a local index without checking any declaration collection.
            /// Panics if the index exceeds the HIR slot capacity.
            pub fn new(index: usize) -> Self {
                Self(u32::try_from(index).expect("HIR declaration capacity exhausted"))
            }

            /// Returns the zero-based slot, without checking a destination collection.
            pub fn index(self) -> usize {
                self.0 as usize
            }
        }
    };
}

/// Identity of one immutable lowering, not a function slot or memory address.
///
/// Lowering source containing `fn a() {} fn b() {}` assigns one identity A to both
/// functions' body-node IDs and the shared Body/SourceMap. Lowering it again
/// obtains another identity B; cloned/reused lowering retains A. A checked process-
/// wide counter creates the value. The nodes use ordinary owned vectors, not an
/// allocator arena. This identity has no source token and no guarantee of persistence
/// across edits, processes or serialized programs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LoweringId(u64);

impl Default for LoweringId {
    fn default() -> Self {
        static NEXT_ARENA: AtomicU64 = AtomicU64::new(1);
        Self(
            NEXT_ARENA
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                .expect("HIR arena identity exhausted"),
        )
    }
}

/// Generates a lowering/owner-qualified index for body nodes or inline binding records.
/// The index addresses its category's storage: function-local for parameters,
/// lowering-wide for body nodes and local binding source slots.
macro_rules! local_id_newtype {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub struct $name {
            arena: LoweringId,
            owner: HirOwner,
            index: u32,
        }

        impl $name {
            pub(crate) fn new(arena: LoweringId, owner: HirOwner, index: usize) -> Self {
                Self {
                    arena,
                    owner,
                    index: u32::try_from(index).expect("HIR arena capacity exhausted"),
                }
            }

            /// Returns the identity of the lowering that allocated this handle.
            pub fn arena(self) -> LoweringId {
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

/// Function or constant whose construction owns a group of lowered nodes.
///
/// `fn run() { 1 }` uses Function(f) for parameter/signature/body node allocation;
/// `const N: i32 = 1;` uses Const(c) for its initializer. f/c select the matching
/// Module.functions/consts row. These are plain local declaration IDs, not runtime
/// frames. An associated constant still uses Const(c), with a separate ConstOwner
/// linking its declaration to the trait/impl.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BodyOwner {
    /// Nodes belonging to a function, including its parameters and body.
    Function(FunctionId),
    /// Nodes belonging to a constant initializer.
    Const(ConstId),
    /// Nodes belonging to a runtime static initializer.
    Static(StaticId),
}

/// Allocation context stored in qualified node IDs and matching vector rows.
///
/// | Source being lowered | Owner |
/// | --- | --- |
/// | Field type in `struct S { val x: i32 }` | `Declaration` |
/// | Parameter type/body in `fn f(x: i32) { ... }` | `Body(Function(f_id))` |
/// | Initializer in `const N: i32 = 1;` | `Body(Const(c_id))` |
///
/// Lowering switches context, constructs children, then restores the previous
/// owner. Declaration is a valid owner, not a missing-function error. Owner tags
/// partition the shared node vectors logically; body-node indices are not relative
/// to that owner. Parameter indices select the owning function's inline list.
/// This allocation context is distinct from FieldId/VariantId's
/// enclosing declaration and ConstItem's trait/impl container.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum HirOwner {
    /// Declaration syntax outside a function/constant body context.
    #[default]
    Declaration,
    /// Nodes allocated while lowering the indicated function or constant.
    Body(BodyOwner),
}

id_newtype!(
    /// A lowering-local index identifying authored declarations in `Module.uses`.
    /// One grouped or recovered use has one ID, independently of its import leaves.
    UseId
);
id_newtype!(
    /// A lowering-local index identifying `Module.functions`.
    /// See the [identity families](crate::hir::ids) for allocation and validity rules.
    FunctionId
);
id_newtype!(
    /// A lowering-local index identifying `Module.consts`.
    /// See the [identity families](crate::hir::ids) for allocation and validity rules.
    ConstId
);
id_newtype!(
    /// A lowering-local index identifying `Module.statics`.
    /// See the [identity families](crate::hir::ids) for allocation and validity rules.
    StaticId
);
id_newtype!(
    /// A lowering-local index identifying `Module.traits`.
    /// See the [identity families](crate::hir::ids) for allocation and validity rules.
    TraitId
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
    /// A function-local position in `Function.params`, qualified by lowering and function owner.
    /// See the [identity families](crate::hir::ids) for allocation and lookup rules.
    ParamId
);
local_id_newtype!(
    /// A lowering/owner-qualified handle identifying a provisional local in `HirNodes.locals`.
    /// See the [identity families](crate::hir::ids) for allocation and lookup rules.
    LocalId
);
id_newtype!(
    /// A lowering-local index identifying `Module.structs`.
    /// See the [identity families](crate::hir::ids) for allocation and validity rules.
    StructId
);
id_newtype!(
    /// A lowering-local index identifying `Module.native_types`.
    /// See the [identity families](crate::hir::ids) for allocation and validity rules.
    NativeTypeId
);
id_newtype!(
    /// A lowering-local index identifying `Module.type_aliases`; never a nominal type identity.
    TypeAliasId
);
id_newtype!(
    /// A lowering-local index identifying `Module.enums`.
    /// See the [identity families](crate::hir::ids) for allocation and validity rules.
    EnumId
);
local_id_newtype!(
    /// A lowering/owner-qualified handle identifying `HirNodes.blocks`.
    /// See the [identity families](crate::hir::ids) for allocation and lookup rules.
    BlockId
);
local_id_newtype!(
    /// A lowering/owner-qualified handle identifying `HirNodes.exprs`.
    /// See the [identity families](crate::hir::ids) for allocation and lookup rules.
    ExprId
);
local_id_newtype!(
    /// A lowering/owner-qualified handle identifying `HirNodes.places`.
    /// See the [identity families](crate::hir::ids) for allocation and lookup rules.
    PlaceId
);
local_id_newtype!(
    /// A lowering/owner-qualified handle identifying `HirNodes.stmts`.
    /// See the [identity families](crate::hir::ids) for allocation and lookup rules.
    StmtId
);
local_id_newtype!(
    /// A lowering/owner-qualified handle identifying `HirNodes.patterns`.
    /// See the [identity families](crate::hir::ids) for allocation and lookup rules.
    PatternId
);
local_id_newtype!(
    /// A lowering/owner-qualified handle identifying `HirNodes.types` (type syntax, not a resolved semantic type).
    /// See the [identity families](crate::hir::ids) for allocation and lookup rules.
    TypeRefId
);
id_newtype!(
    /// A lowering-local index identifying a binder in `Module.generic_params`.
    /// See the [identity families](crate::hir::ids) for allocation and validity rules.
    GenericParamId
);

/// Internal analysis selection, not a source declaration or a runtime function ID.
///
/// For `fn a() {} fn b() {}`, All selects both function bodies and Function(a_id)
/// selects a for a body query; signature/constant prerequisites are prepared
/// separately. The enclosed FunctionId belongs to the matching module lowering.
#[derive(Debug, Clone, Copy)]
pub(crate) enum BodySelection {
    /// Prepare constant prerequisites without checking runtime initializers or functions.
    Constants,
    /// Check all function bodies in the prepared module.
    All,
    /// Check the selected function slot after preparing its analysis prerequisites.
    Function(FunctionId),
}

impl BodySelection {
    pub(crate) fn includes(self, id: FunctionId) -> bool {
        matches!(self, Self::All) || matches!(self, Self::Function(selected) if selected == id)
    }
}

/// Generates a lowering-qualified member slot with a typed declaration owner.
/// Unlike node indices, slot is relative to the enclosing declaration's member list.
macro_rules! member_id {
    ($(#[$meta:meta])* $name:ident, $owner:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub struct $name {
            arena: LoweringId,
            owner: $owner,
            slot: u32,
        }

        impl $name {
            pub(crate) fn new(arena: LoweringId, owner: $owner, slot: usize) -> Self {
                Self {
                    arena,
                    owner,
                    slot: u32::try_from(slot).expect("HIR member capacity exhausted"),
                }
            }

            /// Returns the identity of the lowering that allocated this handle.
            pub fn arena(self) -> LoweringId {
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

/// Declaration whose inline field list owns a field identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FieldOwner {
    /// An ordinary script struct declaration.
    Struct(StructId),
    /// A Native type's declared interface, independent of runtime layout.
    NativeType(NativeTypeId),
}

member_id!(
    /// A lowering-qualified method slot in its enclosing trait.
    TraitMethodId,
    TraitId
);
member_id!(
    /// A lowering-qualified field slot within its struct or Native declaration.
    /// See the [identity families](crate::hir::ids) for member lookup.
    FieldId,
    FieldOwner
);
member_id!(
    /// A lowering-qualified enum variant slot within its enclosing declaration.
    /// See the [identity families](crate::hir::ids) for member lookup.
    VariantId,
    EnumId
);
