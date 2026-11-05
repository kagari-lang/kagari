//! Short-lived checked operations. Source IDs address authored structure; operation
//! indices additionally distinguish synthetic conversions at the same source site.
//!
//! For `fn add(x: i32) -> i32 { x + 1 }`, `CheckedBody::blocks` retains the root
//! block and its tail source ID. `results[tail]` selects an `Operator` operation;
//! its applied call stores the selected method, signature and `[x, one]` operands.
//! The operands address a parameter read and a decoded integer constant.
//!
//! A conversion can append another operation with the same source ID/span. The
//! `values` map addresses the original value, while `results` addresses the final
//! conversion. Source IDs remain owner/arena qualified; operation indices are
//! local to this one short-lived expansion. Neither is a cross-revision key.
//!
//! Structural children describe conditional execution, not a flat instruction
//! stream. Planning/lowering must walk the root, preserve branches and stop at
//! terminating operands; iterating `operations` is not an execution schedule.

use kagari_common::{identity::table::DefinitionId, span::Span};
use kagari_contract::numeric::NumericConversion;
use kagari_hir::{
    builtin::BuiltinFunction,
    callable::AppliedCallSignature,
    compile_input::body::{BodyKey, BodyRoot, bindings::BindingId},
    hir::{
        expr::{
            Condition,
            ops::{BinaryOp, PrefixOp},
        },
        ids::{BlockId, ExprId, LocalId, PatternId, PlaceId, StmtId},
    },
    typeck::{
        scalar::ScalarValue,
        table::{
            ResolvedAssociatedConst, ResolvedHostPath, ResolvedHostPlacePath,
            ResolvedInterfaceCoercion, ResolvedIteration, propagation::ResolvedPropagation,
        },
    },
    types::{NominalType, TypeId},
};
use std::collections::{HashMap, HashSet};

/// An operation index belongs to one expanded body, never to a source arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OperationId(pub(super) usize);

/// Expanded value with a complete semantic type and an authored source extent.
#[derive(Debug, Clone)]
pub struct CheckedExpr {
    pub source: ExprId,
    pub span: Span,
    pub ty: TypeId,
    pub kind: CheckedExprKind,
}

/// A selected callable contract; type arguments may still refer to enclosing binders.
#[derive(Debug, Clone)]
pub struct CheckedCall {
    pub target: CheckedCallTarget,
    pub signature: AppliedCallSignature,
    /// Optional receiver followed by explicit arguments, each evaluated once.
    pub operands: Vec<ExprId>,
}

/// How authored operands are supplied to an already selected trait method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MethodOperands {
    /// A written method call already supplies the declaration's parameter structure.
    Ordinary,
    /// `f(a, b)` through `Fn` supplies separate operands; planning packs `(a, b)`
    /// for a trait implementation, or calls a concrete closure directly.
    Callable,
}

/// Declaration selection is complete; instance-dependent dispatch belongs to planning.
#[derive(Debug, Clone)]
pub enum CheckedCallTarget {
    Direct {
        declaration: DefinitionId,
        arguments: Vec<TypeId>,
    },
    /// Retains the proved application and exact member, without another spelling lookup.
    Method {
        operands: MethodOperands,
        method: DefinitionId,
        interface: NominalType,
        receiver: TypeId,
        arguments: Vec<TypeId>,
    },
    /// Callee is evaluated once before the operands; it is not an argument.
    Value { callee: ExprId },
}

/// Explicit synthetic conversion at the original expression's source position.
#[derive(Debug, Clone)]
pub enum Conversion {
    Callable {
        receiver: TypeId,
        interface: NominalType,
    },
    Interface(ResolvedInterfaceCoercion),
    /// Records a readonly capability on the result without changing aliases.
    ReadOnly,
}

/// A runtime value location, with imported statics reduced to their canonical identity.
/// Constants are emitted as scalar operations; type/function/namespace names cannot
/// be smuggled into this representation as ordinary reads or mutation roots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueBinding {
    Lexical(BindingId),
    Static(DefinitionId),
}

/// Executable expression meaning, with names/fields/constructors already selected.
#[derive(Debug, Clone)]
pub enum CheckedExprKind {
    Constant(ScalarValue),
    Read(ValueBinding),
    FunctionValue {
        declaration: DefinitionId,
        arguments: Vec<TypeId>,
    },
    AssociatedConstant(ResolvedAssociatedConst),
    Call(CheckedCall),
    Helper {
        helper: BuiltinFunction,
        operands: Vec<ExprId>,
    },
    /// Evaluate this source-order prefix; its last operand exits before the parent operation.
    Diverging(Vec<ExprId>),
    Convert {
        value: OperationId,
        conversion: Conversion,
    },
    NumericCast {
        value: ExprId,
        conversion: NumericConversion,
    },
    /// Operator identity is explicit; intrinsic/instance representation is planned later.
    Operator {
        call: CheckedCall,
        syntax: OperatorSyntax,
    },
    Logical {
        lhs: ExprId,
        op: BinaryOp,
        rhs: ExprId,
    },
    Unary {
        operand: ExprId,
        op: PrefixOp,
    },
    PrimitiveBinary {
        lhs: ExprId,
        op: BinaryOp,
        rhs: ExprId,
    },
    /// PartialEq selection depends on the instantiated operand type, including composition.
    Equality {
        lhs: ExprId,
        rhs: ExprId,
        negate: bool,
        receiver: TypeId,
    },
    /// A string formatting segment already has the required string representation.
    Identity(ExprId),
    Field {
        receiver: ExprId,
        field: DefinitionId,
    },
    Index {
        receiver: ExprId,
        index: ExprId,
        operation: IndexRead,
    },
    HostRead(ResolvedHostPath),
    Struct {
        declaration: DefinitionId,
        fields: Vec<(DefinitionId, ExprId)>,
        base: Option<ExprId>,
        base_fields: Vec<DefinitionId>,
    },
    Enum {
        declaration: DefinitionId,
        variant: DefinitionId,
        fields: Vec<ExprId>,
    },
    Tuple(Vec<ExprId>),
    Array(Vec<ExprId>),
    ArrayRepeat {
        value: ExprId,
        count: ExprId,
    },
    Range {
        start: Option<ExprId>,
        end: Option<ExprId>,
        inclusive: bool,
    },
    InterpolatedString(Vec<ExprId>),
    Format {
        call: CheckedCall,
    },
    Await {
        future: ExprId,
    },
    Propagate {
        value: ExprId,
        protocol: Box<ResolvedPropagation>,
    },
    Block(BlockId),
    If {
        condition: Condition,
        then_branch: BlockId,
        else_branch: Option<ExprId>,
    },
    Match {
        scrutinee: ExprId,
        arms: Vec<CheckedMatchArm>,
    },
    Loop(BlockId),
    Closure {
        key: BodyKey,
        is_async: bool,
        captures: Vec<CheckedCapture>,
    },
    AsyncBlock {
        key: BodyKey,
        captures: Vec<CheckedCapture>,
    },
}

/// A pattern decision followed by its conditional guard and reachable value.
#[derive(Debug, Clone)]
pub struct CheckedMatchArm {
    pub pattern: PatternId,
    pub guard: Option<ExprId>,
    /// Absent when the guard cannot finish evaluating.
    pub value: Option<ExprId>,
}

#[derive(Debug, Clone, Copy)]
pub enum OperatorSyntax {
    Prefix,
    Binary(BinaryOp),
}

#[derive(Debug, Clone)]
pub enum IndexRead {
    Aggregate,
    Method(Box<CheckedCall>),
}

/// One captured lexical binding, with the authored range separate from its use site.
#[derive(Debug, Clone)]
pub struct CheckedCapture {
    pub id: BindingId,
    pub name: String,
    pub span: Span,
    pub ty: TypeId,
    /// Mutable locals share a cell across the owner and every capturing callable.
    pub by_cell: bool,
}

#[derive(Debug, Clone)]
pub struct CheckedLocal {
    pub id: LocalId,
    pub span: Span,
    pub name: String,
    pub ty: TypeId,
}

#[derive(Debug, Clone)]
pub struct CheckedBlock {
    pub statements: Vec<StmtId>,
    pub tail: Option<ExprId>,
}

#[derive(Debug, Clone)]
pub struct CheckedStmt {
    pub span: Span,
    pub kind: CheckedStmtKind,
}

#[derive(Debug, Clone)]
pub enum CheckedStmtKind {
    Function,
    Binding {
        pattern: PatternId,
        initializer: ExprId,
    },
    Assign {
        target: PlaceId,
        op: Option<BinaryOp>,
        value: ExprId,
    },
    /// Target preparation exits before any RHS or mutation operation.
    EvaluatePlace(PlaceId),
    /// Prepare the target, then evaluate an RHS that exits before commit.
    EvaluateAssignment {
        target: PlaceId,
        value: ExprId,
    },
    Return(Option<ExprId>),
    While {
        condition: Condition,
        body: BlockId,
    },
    Loop(BlockId),
    For {
        pattern: PatternId,
        iterable: ExprId,
        body: BlockId,
        iteration: Box<ResolvedIteration>,
    },
    Break,
    BreakValue(ExprId),
    Continue,
    Expr(ExprId),
}

#[derive(Debug, Clone)]
pub struct CheckedPlace {
    /// Stored value type, or `Never` when preparation produces no location.
    pub ty: TypeId,
    pub kind: CheckedPlaceKind,
}

#[derive(Debug, Clone)]
pub enum CheckedPlaceKind {
    Binding(ValueBinding),
    Value(ExprId),
    Field {
        base: PlaceId,
        field: DefinitionId,
    },
    Index {
        base: PlaceId,
        index: ExprId,
        operation: IndexWrite,
    },
    Host(ResolvedHostPlacePath),
    /// Preparation's evaluated prefix, with no resulting location or access operation.
    Diverging {
        base: Option<PlaceId>,
        /// Host roots and read-only index receivers are read before dynamic operands.
        read_base: bool,
        values: Vec<ExprId>,
    },
}

#[derive(Debug, Clone)]
pub enum IndexWrite {
    Aggregate,
    ReadMethod(NominalType),
    Method {
        read: NominalType,
        write: NominalType,
        setter: DefinitionId,
    },
}

#[derive(Debug, Clone)]
pub struct CheckedPattern {
    pub irrefutable: bool,
    pub kind: CheckedPatternKind,
}

#[derive(Debug, Clone)]
pub enum CheckedPatternKind {
    Wildcard,
    Binding(CheckedLocal),
    Constant(ScalarValue),
    Range {
        start: ScalarValue,
        end: ScalarValue,
        inclusive: bool,
    },
    Or(Vec<PatternId>),
    Tuple(Vec<PatternId>),
    Struct(Vec<(DefinitionId, PatternId)>),
    Variant {
        declaration: DefinitionId,
        fields: Vec<PatternId>,
    },
}

/// Creation contract of a nested callable whose root is being expanded.
///
/// Its type is the closure's callable type or the async block's Future type;
/// result specialization and generated resume identities belong to planning.
#[derive(Debug, Clone)]
pub struct CallableBody {
    pub span: Span,
    pub ty: TypeId,
    pub is_async: bool,
    pub is_block: bool,
    pub parameters: Vec<CheckedLocal>,
}

/// One expanded root and its structural descendants. Nested callable bodies are
/// expanded separately, so retaining a closure does not duplicate its owner's arena.
/// No analysis snapshot, type table or syntax resolver survives in this result.
#[derive(Debug)]
pub struct CheckedBody {
    pub key: BodyKey,
    pub root: BodyRoot,
    /// Present only when this expansion starts at a closure or async block.
    pub callable: Option<CallableBody>,
    /// Incoming bindings for a nested callable, in the resolver's capture order.
    pub captures: Vec<CheckedCapture>,
    /// Locals requiring cells in this root, including incoming cell captures.
    pub cell_locals: HashSet<LocalId>,
    pub operations: Vec<CheckedExpr>,
    /// Raw source-value operation, before implicit conversions.
    pub values: HashMap<ExprId, OperationId>,
    /// Final operation produced for each authored value expression.
    pub results: HashMap<ExprId, OperationId>,
    pub blocks: HashMap<BlockId, CheckedBlock>,
    pub statements: HashMap<StmtId, CheckedStmt>,
    pub places: HashMap<PlaceId, CheckedPlace>,
    pub patterns: HashMap<PatternId, CheckedPattern>,
}

impl CheckedBody {
    pub fn value(&self, id: ExprId) -> Option<&CheckedExpr> {
        self.operations.get(self.values.get(&id)?.0)
    }

    pub fn result(&self, id: ExprId) -> Option<&CheckedExpr> {
        self.operations.get(self.results.get(&id)?.0)
    }

    pub fn operation(&self, id: OperationId) -> &CheckedExpr {
        &self.operations[id.0]
    }
}
