//! Expressions and their stored child IDs, before name resolution and type checking.

pub mod literal;
use crate::hir::{
    expr::{
        literal::Literal,
        ops::{BinaryOp, PrefixOp},
    },
    ids::{BlockId, ExprId, LocalId, PatternId, TypeRefId},
    ty::TypeBuffer,
};

pub mod ops;

use smallvec::SmallVec;

/// An expression payload addressed by an [`ExprId`] in the shared body arena.
///
/// Only `kind` is stored here. Names, types, selected calls and source sites belong
/// to separate analysis/source tables. See [`crate::hir::body::Body`] for the
/// binary-expression storage diagram and arena/owner lookup checks.
#[derive(Debug, Clone)]
pub struct ExprData {
    /// Expression form and stored child IDs, before semantic checking.
    pub kind: ExprKind,
}

/// Source-level expression forms; children are IDs, not owned recursive boxes.
///
/// # Calls and qualified names
///
/// ```text
/// add::<i32>(41)
/// Call { callee: n, args: [v], type_args: Some([t]) }
/// +-- n -> Body.expr -> Name { name: "add", explicit_type: None }
/// +-- v -> Body.expr -> Literal(Number, "41")   // literal payload abbreviated
/// `-- t -> Body.type_ref -> Named("i32")
///
/// m::nested::value()
/// Call { callee: p, args: [], type_args: None }
/// `-- p -> Name { name: "m::nested::value", explicit_type: None }
/// ```
///
/// Path components are retained as spelling and source-map sites, not a chain of
/// synthetic imports. Receiver/type qualification can instead populate
/// `Name.explicit_type`. A dot call has a `Field` callee with a receiver expression;
/// resolution and type checking determine what it calls.
///
/// # Conditional and aggregate shapes
///
/// ```text
/// if ready { value } else { fallback }
/// If { condition: Expr(c), then_branch: b, else_branch: Some(e) }
/// +-- c -> Name("ready")                 // payloads abbreviated
/// +-- b -> BlockData { tail_expr: Some(v), ... }
/// `-- e -> Block(other_block)
///
/// match pair { (x, _) => x }
/// Match { scrutinee: pair_id, arms: [MatchArm { pattern: p, guard: None, expr: x_id }] }
/// `-- p -> PatternKind::Tuple([binding, wildcard])
///
/// |x: i32| x + 1
/// Closure { params: [ClosureParam { local: l, ty: Some(t), ... }], body: sum }
/// ```
///
/// `MatchArm`, `FieldInit` and `ClosureParam` are inline records inside the owning
/// expression; their child IDs enter the matching body vectors. Parentheses do not
/// allocate an extra expression node. `Missing` keeps recovered trees traversable,
/// but does not make them valid checked input.
#[derive(Debug, Clone)]
pub enum ExprKind {
    /// Placeholder for absent or cancelled expression lowering.
    Missing,
    /// An explicit `value as Type` conversion.
    Cast {
        /// Source value expression.
        expr: ExprId,
        /// Target type syntax; the selected conversion is a later fact.
        target: TypeRefId,
    },
    /// Ordered literal-text and formatting-part expression IDs from an interpolated string.
    InterpolatedString(ExprBuffer),
    /// One expression hole within an interpolated string.
    FormatPart {
        /// Value to format.
        expr: ExprId,
        /// Whether the source requested debug formatting.
        debug: bool,
    },
    /// An unresolved name/path, optionally carrying an explicit receiver/type application.
    Name {
        /// Name/path spelling, including `::` components.
        name: String,
        /// Explicit type qualification/application when present, not the inferred expression type.
        explicit_type: Option<TypeRefId>,
    },
    /// A literal spelling and lexical category; scalar interpretation happens later.
    Literal(Literal),
    /// Postfix `value?`, before Try/FromResidual protocol selection.
    Propagate {
        /// Expression producing the value to branch or propagate.
        expr: ExprId,
    },
    /// A unary `-value` or `!value` operation.
    Prefix {
        /// Source unary operator.
        op: PrefixOp,
        /// Operand expression.
        expr: ExprId,
    },
    /// An infix expression such as `x + 1` or `a && b`.
    Binary {
        /// Left operand expression.
        lhs: ExprId,
        /// Source binary operator; checked operation facts are separate.
        op: BinaryOp,
        /// Right operand expression.
        rhs: ExprId,
    },
    /// A range such as `1..3`, `1..=3` or `..end`.
    Range {
        /// Start bound, absent for an open start.
        start: Option<ExprId>,
        /// End bound, absent for an open end.
        end: Option<ExprId>,
        /// Whether the source uses `..=` rather than `..`.
        inclusive: bool,
    },
    /// A call whose callee and argument expressions are retained independently.
    Call {
        /// Callee expression, which can be a name, member or value expression.
        callee: ExprId,
        /// Argument expressions in source order.
        args: ExprBuffer,
        /// Explicit call-site type arguments; `None` means no such syntax.
        type_args: Option<TypeBuffer>,
    },
    /// Member access `receiver.name`; callable member selection happens later.
    Field {
        /// Expression supplying the receiver.
        receiver: ExprId,
        /// Member spelling, unresolved at this stage.
        name: String,
    },
    /// Read access `receiver[index]`; assignment uses a separate place form.
    Index {
        /// Indexed value expression.
        receiver: ExprId,
        /// Index expression.
        index: ExprId,
    },
    /// An `if` expression with an optional `else` branch.
    If {
        /// Plain or pattern-binding condition.
        condition: Condition,
        /// Then block ID; recovery can supply an empty block.
        then_branch: BlockId,
        /// Else expression, including nested `if` or block expressions; absent when omitted.
        else_branch: Option<ExprId>,
    },
    /// A `match` expression with source-ordered alternatives.
    Match {
        /// Value being matched.
        scrutinee: ExprId,
        /// Inline pattern/guard/body records, in source order.
        arms: MatchArmBuffer,
    },
    /// A value-position unconditional `loop` expression.
    Loop {
        /// Loop block; `break value` can determine its result type.
        body: BlockId,
    },
    /// A closure such as `|x| x + 1`.
    Closure {
        /// Inline parameters whose identities are local bindings.
        params: Vec<ClosureParam>,
        /// Expression body; a braced body is an `ExprKind::Block`.
        body: ExprId,
    },
    /// A named-field constructor such as `Point { x: 1 }`.
    StructInit {
        /// Unresolved constructor path.
        path: String,
        /// Explicit generic application when supplied.
        explicit_type: Option<TypeRefId>,
        /// Inline named-field values in source order.
        fields: FieldInitBuffer,
    },
    /// Tuple elements such as `(x, y)`, each retained as an expression ID.
    Tuple(ExprBuffer),
    /// Array literal elements such as `[x, y]`, before library construction selection.
    Array(ExprBuffer),
    /// Repeated array construction `[value; count]`.
    ArrayRepeat {
        /// Element value expression.
        value: ExprId,
        /// Repeat-count expression, not a type-level array length.
        count: ExprId,
    },
    /// A braced expression linking to a separate `BlockData` row.
    Block(BlockId),
}

/// An ordinary boolean condition or a pattern-binding condition.
///
/// ```text
/// if val Some(x) = value { x }
/// Condition::Binding { pattern: p, initializer: e }
/// +-- p -> PatternKind::EnumVariant { fields: [binding_x], ... }
/// `-- e -> ExprKind::Name { name: "value", ... }
/// ```
///
/// The condition is embedded in an `If`/`While`, not separately arena allocated.
/// Bound names are introduced by resolution in the applicable success scope.
#[derive(Debug, Clone)]
pub enum Condition {
    /// An ordinary condition expression whose boolean type is checked later.
    Expr(ExprId),
    /// A binding condition with a pattern and initializer.
    Binding {
        /// Pattern tested against the initializer and introducing local IDs.
        pattern: PatternId,
        /// Expression supplying the matched value.
        initializer: ExprId,
    },
}

impl Condition {
    /// Returns the tested expression, including the initializer of a binding condition.
    pub fn value(&self) -> ExprId {
        match self {
            Self::Expr(expr) => *expr,
            Self::Binding { initializer, .. } => *initializer,
        }
    }
}

/// A source-ordered match alternative stored inline in `ExprKind::Match`.
#[derive(Debug, Clone)]
pub struct MatchArm {
    /// Pattern to test and bind before evaluating the guard/body.
    pub pattern: PatternId,
    /// Optional `if` guard expression.
    pub guard: Option<ExprId>,
    /// Expression evaluated for the selected arm.
    pub expr: ExprId,
}

/// A named constructor field stored inline, such as `x: value` or shorthand `x`.
#[derive(Debug, Clone)]
pub struct FieldInit {
    /// Source field name, resolved against the constructor type later.
    pub name: String,
    /// Value expression; shorthand also lowers to a `Name` expression.
    pub value: ExprId,
}

/// A closure parameter stored inline in its owning expression.
#[derive(Debug, Clone)]
pub struct ClosureParam {
    /// Parameter name as written.
    pub name: String,
    /// Local binding identity, unlike a declared function's `ParamId`.
    pub local: LocalId,
    /// Explicit type annotation, or absent for inference.
    pub ty: Option<TypeRefId>,
}

/// Source-ordered expression IDs with four inline slots, not a four-element limit.
pub type ExprBuffer = SmallVec<[ExprId; 4]>;
/// Source-ordered inline match arms with four inline slots.
pub type MatchArmBuffer = SmallVec<[MatchArm; 4]>;
/// Source-ordered inline constructor fields with four inline slots.
pub type FieldInitBuffer = SmallVec<[FieldInit; 4]>;
