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
            Condition, MatchArm,
            ops::{BinaryOp, PrefixOp},
        },
        ids::{BlockId, ExprId, FunctionId, LocalId, PatternId, PlaceId, StmtId},
        writeability::Writeability,
    },
    resolver::resolved::ResolvedName,
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

/// Declaration selection is complete; instance-dependent dispatch belongs to planning.
#[derive(Debug, Clone)]
pub enum CheckedCallTarget {
    Direct {
        declaration: DefinitionId,
        arguments: Vec<TypeId>,
    },
    /// Retains the proved application and exact member, without another spelling lookup.
    Method {
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

/// Executable expression meaning, with names/fields/constructors already selected.
#[derive(Debug, Clone)]
pub enum CheckedExprKind {
    Constant(ScalarValue),
    Read(ResolvedName),
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
    /// The callee terminates; explicit argument syntax contributes no operations.
    TerminatingCallee(ExprId),
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
        value: ExprId,
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
        arms: Vec<MatchArm>,
    },
    Loop(BlockId),
    Closure {
        key: BodyKey,
        is_async: bool,
        captures: Vec<CheckedCapture>,
        parameters: Vec<CheckedLocal>,
    },
    AsyncBlock {
        key: BodyKey,
        captures: Vec<CheckedCapture>,
    },
}

#[derive(Debug, Clone, Copy)]
pub enum OperatorSyntax {
    Prefix(PrefixOp),
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
    pub span: Span,
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
    Function(FunctionId),
    Binding {
        pattern: PatternId,
        writeability: Writeability,
        initializer: ExprId,
    },
    Assign {
        target: PlaceId,
        op: Option<BinaryOp>,
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
    pub span: Span,
    pub ty: TypeId,
    pub kind: CheckedPlaceKind,
}

#[derive(Debug, Clone)]
pub enum CheckedPlaceKind {
    Binding(ResolvedName),
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
    pub span: Span,
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

/// One expanded root and its structural descendants. Nested callable bodies are
/// expanded separately, so retaining a closure does not duplicate its owner's arena.
/// No analysis snapshot, type table or syntax resolver survives in this result.
#[derive(Debug)]
pub struct CheckedBody {
    pub key: BodyKey,
    pub root: BodyRoot,
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
