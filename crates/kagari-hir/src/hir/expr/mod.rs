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

/// One expression payload in Body.exprs, addressed by a qualified ExprId.
///
/// ```text
/// x + 1
/// e -> Body.expr(e) -> ExprData { kind: Binary { lhs: x_id, op: Add, rhs: one_id } }
/// x_id -> ExprData { kind: Name { name: "x", explicit_type: None } }
/// one_id -> ExprData { kind: Literal(Literal { kind: Number, text: "1" }) }
/// ```
///
/// Only `kind` is stored here. The ID/row supply arena and owner; SourceMap
/// supplies byte sites; ResolvedNames and TypeTable supply binding/type/call facts.
/// Expression lowering builds the rows and child links before checking. See
/// ExprKind for every source shape, and Body for checked lookup and physical storage.
#[derive(Debug, Clone)]
pub struct ExprData {
    /// Expression form and stored child IDs, before semantic checking.
    pub kind: ExprKind,
}

/// Expression syntax with ID-linked children and inline small member records.
///
/// In these source-to-field mappings, `e_x` denotes the ExprId of source `x`,
/// `t_i32` denotes its TypeRefId, and `b` denotes a BlockId. IDs are symbolic;
/// use Body::expr/type_ref/block/pattern to follow the appropriate link. These
/// are context-dependent source fragments, not claims of successful type checking.
///
/// # Names, calls and projections
///
/// ```text
/// x                    -> Name { name: "x", explicit_type: None }
/// m::run               -> Name { name: "m::run", explicit_type: None }
/// Pair<i32>::make      -> Name { name: "Pair::make", explicit_type: Some(t_pair) }
/// t_pair               -> Generic { name: "Pair", args: [t_i32], bindings: [],
///                                   positional_after_binding: false, callable_syntax: false }
/// add(41)              -> Call { callee: e_add, args: [e_41], type_args: None }
/// add::<i32>(41)       -> Call { callee: e_add, args: [e_41], type_args: Some([t_i32]) }
/// object.run(41)       -> Call { callee: e_member, args: [e_41], type_args: None }
/// e_member             -> Field { receiver: e_object, name: "run" }
/// items[i]             -> Index { receiver: e_items, index: e_i }
/// ```
///
/// A callee is an expression, not a FunctionId: it can name a free function,
/// select a method, or evaluate a closure. `explicit_type` qualifies the name's
/// receiver/type application; `type_args` belongs to the CALL. Neither stores
/// the callee's checked type. Path component sites live in SourceMap; member
/// and callable identities are selected by resolution/type checking.
///
/// # Values and operations
///
/// | Source | Kind (all payload fields shown) |
/// | --- | --- |
/// | `42` | `Literal(Literal { kind: Number, text: "42" })` |
/// | `x as i32` | `Cast { expr: e_x, target: t_i32 }` |
/// | `future.await` / `result?` | `Await { expr: e_future }` / `Propagate { expr: e_result }` |
/// | `-x` / `!ready` | `Prefix { op: Neg, expr: e_x }` / `Prefix { op: Not, expr: e_ready }` |
/// | `x + y` / `a && b` | `Binary { lhs: e_x, op: Add, rhs: e_y }` / same fields with AndAnd |
/// | `1..=end` | `Range { start: Some(e_1), end: Some(e_end), inclusive: true }` |
/// | `..end` / `start..` | Range with start/end absent respectively, inclusive false |
/// | `(x, y)` / `()` | `Tuple([e_x, e_y])` / `Tuple([])` |
/// | `[x, y]` / `[]` | `Array([e_x, e_y])` / `Array([])` |
/// | `[x; n]` | `ArrayRepeat { value: e_x, count: e_n }` |
/// | `{ x }` | `Block(b)`, with BlockData { statements: [], tail_expr: Some(e_x) } |
///
/// Grouping `(x)` returns e_x directly. The presence of child links does not
/// imply all children always execute: short-circuiting and propagation retain
/// their checked control-flow semantics. Range absence is legal open-bound syntax.
///
/// ```text
/// f"count={n}, value={v:?}"
/// -> InterpolatedString([text0, part0, text1, part1])
/// text0 -> Literal(Literal { kind: String, text: "\"count=\"" })
/// part0 -> FormatPart { expr: e_n, debug: false }
/// text1 -> Literal(Literal { kind: String, text: "\", value=\"" })
/// part1 -> FormatPart { expr: e_v, debug: true }
/// ```
///
/// Text chunks and FormatPart rows are synthesized by lowering, in source order;
/// escaped doubled braces become literal text. Formatting protocol selection is
/// a later fact, not a method call already present in these nodes.
///
/// # Branches, constructors and closures
///
/// ```text
/// if ready { x } else { y }
/// -> If { condition: Condition::Expr(e_ready), then_branch: b_x, else_branch: Some(e_else) }
/// e_else -> Block(b_y); b_x/b_y -> BlockData with x/y tails
/// if ready { work(); } -> same If, else_branch: None
///
/// match value { Some(x) if ready => x, _ => 0 }
/// -> Match { scrutinee: e_value, arms: [some_arm, fallback_arm] }
/// some_arm = MatchArm { pattern: p_some, guard: Some(e_ready), expr: e_x }
/// fallback_arm = MatchArm { pattern: p_wildcard, guard: None, expr: e_0 }
/// p_some -> PatternKind::EnumVariant { path: "Some", fields: [p_x] }
///
/// loop { break 1; } in value position -> Loop { body: b }
/// b -> BlockData { statements: [break_stmt], tail_expr: None }
/// break_stmt -> StmtKind::BreakValue(e_1)
///
/// |x: i32| x + 1
/// -> Closure { is_async: false, params: [ClosureParam { name: "x", local: l, ty: Some(t_i32) }],
///              body: e_sum }
/// async |x: i32| { x } -> Closure with is_async true, body pointing to Block(b_x)
///
/// Point { x: 1, y }
/// -> StructInit { path: "Point", explicit_type: None,
///                 fields: [FieldInit { name: "x", value: e_1 }, FieldInit { name: "y", value: e_y }] }
/// e_y -> Name { name: "y", explicit_type: None } // synthesized shorthand
/// Point<i32> { x: 1 } -> same StructInit with explicit_type: Some(t_point)
/// ```
///
/// MatchArm, FieldInit and ClosureParam are INLINE records, not extra arena rows.
/// Their children enter expression/pattern/type tables. Closure captures are
/// discovered later; the Closure payload has no capture list. An unannotated
/// parameter stores `ty: None` and requires contextual typing; `||` has no params.
/// A braced closure body uses an expression link to Block, while If/Loop directly
/// link to blocks. A nested `else if` is another If expression rather than a block.
///
/// Missing has no payload and no valid source spelling. Incomplete syntax or
/// cancelled lowering can allocate it to retain a traversable recoverable tree;
/// it does not make the source acceptable for executable lowering.
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
    /// Explicit postfix suspension; semantic checking selects the Future output.
    Await {
        /// Expression producing the awaitable.
        expr: ExprId,
    },
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
        /// Whether this closure explicitly carries the `async` modifier.
        is_async: bool,
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

/// An inline boolean or pattern-binding condition, embedded in If/While.
///
/// ```text
/// if ready { work(); } -> Condition::Expr(e_ready)
/// if val Some(x) = value { use_value(x); }
/// -> Condition::Binding { pattern: p, initializer: e_value }
/// p -> Body.pattern -> EnumVariant { path: "Some", fields: [p_x] }
/// p_x -> Name { name: "x", local: l }
/// e_value -> Body.expr -> Name { name: "value", explicit_type: None }
/// ```
///
/// The Condition itself has no ID/vector. Expr holds the tested expression;
/// Binding holds a pattern plus the value to match, not an ordinary binding
/// statement. Resolution introduces x in the success scope; checking supplies
/// boolean/pattern facts. The same shape applies to binding while conditions.
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

/// A source-ordered match alternative stored inline in ExprKind::Match.arms.
///
/// ```text
/// Some(x) if x > 0 => x
/// MatchArm { pattern: p_some, guard: Some(e_positive), expr: e_x }
/// p_some -> Body.pattern -> EnumVariant { path: "Some", fields: [p_x] }
/// e_positive -> Body.expr -> Binary { lhs: e_guard_x, op: Gt, rhs: e_0 }
/// e_x -> Body.expr -> Name { name: "x", explicit_type: None }
/// ```
///
/// A plain `_ => 0` has a Wildcard pattern, `guard: None` and a literal body.
/// A braced arm body stores an expression pointing to a block, not a BlockId
/// directly. Resolution/checking establish bindings before the guard/body;
/// pattern success and guard success select the arm.
#[derive(Debug, Clone)]
pub struct MatchArm {
    /// Pattern to test and bind before evaluating the guard/body.
    pub pattern: PatternId,
    /// Optional `if` guard expression.
    pub guard: Option<ExprId>,
    /// Expression evaluated for the selected arm.
    pub expr: ExprId,
}

/// A named constructor value stored inline in ExprKind::StructInit.fields.
///
/// `Point { x: 1, y }` gives FieldInit { name: "x", value: e_1 } and
/// FieldInit { name: "y", value: e_y }. e_1 points to a number Literal; e_y
/// points to a synthesized Name { name: "y", explicit_type: None } expression.
/// `name` selects the declared field; `value` is an expression link, not a type
/// or FieldId. Type checking supplies selected field identities and diagnostics.
#[derive(Debug, Clone)]
pub struct FieldInit {
    /// Source field name, resolved against the constructor type later.
    pub name: String,
    /// Value expression; shorthand also lowers to a `Name` expression.
    pub value: ExprId,
}

/// An inline closure binder, using LocalId rather than a function ParamId.
///
/// `|x: i32| x + 1` gives ClosureParam { name: "x", local: l, ty: Some(t) },
/// where t enters Body.types as Named("i32") and l keys local binding/type facts.
/// With contextual `fn(i32) -> i32` typing, `|x| x + 1` stores `ty: None`.
/// The ID is allocated; the name/annotation are source syntax. Closure resolution
/// distinguishes parameter bindings from later-discovered captured outer locals.
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
