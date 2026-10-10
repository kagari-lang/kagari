//! Match and binding pattern storage, including provisional local identities.

use crate::hir::{
    expr::literal::Literal,
    ids::{LocalId, PatternId},
};

/// A pattern row in Body.patterns, addressed by PatternId, with `kind` as payload.
///
/// ```text
/// match pair { (x, _) => x }
/// outer -> PatternData { kind: Tuple([p0, p1]) }
/// p0 -> PatternData { kind: Name { name: "x", local: l } }
/// p1 -> PatternData { kind: Wildcard }
/// l -> provisional binding identity used by resolution/type facts
/// ```
///
/// Pattern lowering allocates child patterns and local IDs; no checked constructor
/// or matched-value type lives in this row. Grouping `(x)` collapses to the inner
/// pattern. Recovery can synthesize a Name with `"<missing>"`. Resolution/checking
/// decide bindings and constructor meanings; allocation alone is not validation.
#[derive(Debug, Clone)]
pub struct PatternData {
    /// Pattern structure and binding IDs, before constructor/type resolution.
    pub kind: PatternKind,
}

/// Source patterns and nested pattern links, used by match, for and binding conditions.
///
/// | Pattern source | Kind (all payload fields shown) |
/// | --- | --- |
/// | `_` | `Wildcard` |
/// | `1 \| 2` | `Or([one_pattern, two_pattern])` |
/// | `1..=LIMIT` | `Range { start: Literal(number_one), end: Path("LIMIT"), inclusive: true }` |
/// | `x` | `Name { name: "x", local: l }` |
/// | `42` | `Literal(Literal { kind: Number, text: "42" })` |
/// | `(x, _)` / `()` | `Tuple([x_pattern, wildcard])` / `Tuple([])` |
/// | `Point { x: n, y }` | `Struct { path: "Point", fields: [PatternField { name: "x", pattern: n_pattern }, PatternField { name: "y", pattern: y_pattern }] }` |
/// | `Choice::Some(x)` | `EnumVariant { path: "Choice::Some", fields: [x_pattern] }` |
/// | `Choice::None` | `EnumVariant { path: "Choice::None", fields: [] }` |
///
/// Or/Tuple/variant child IDs enter Body.patterns; struct fields are inline
/// records linking to subpatterns. Range endpoints are inline PatternBound values,
/// not ExprIds, and `..` sets `inclusive: false`. Bare names receive provisional
/// LocalIds: a visible unit variant can later resolve as a constructor instead
/// of a binding. Struct shorthand `y` creates a nested Name { name: "y", local }
/// pattern. Semantic tables own alternative-binding consistency and constructor
/// identity; neither is implied by these strings.
#[derive(Debug, Clone)]
pub enum PatternKind {
    /// `_`, which matches without introducing a name.
    Wildcard,
    /// Alternatives separated by `|`, each addressing another pattern.
    Or(Vec<PatternId>),
    /// A bounded range pattern such as `1..=3`.
    Range {
        /// Literal or constant-path lower bound.
        start: PatternBound,
        /// Literal or constant-path upper bound.
        end: PatternBound,
        /// Whether the upper bound is inclusive (`..=`).
        inclusive: bool,
    },
    /// A bare identifier with a provisional local binding identity.
    Name {
        /// Source identifier, or a recovery placeholder.
        name: String,
        /// Identity used if this name introduces a binding.
        local: LocalId,
    },
    /// A literal pattern whose value/type is checked later.
    Literal(Literal),
    /// Tuple element patterns in source order.
    Tuple(Vec<PatternId>),
    /// A named-field pattern.
    Struct {
        /// Unresolved struct path.
        path: String,
        /// Inline field-name/subpattern pairs.
        fields: Vec<PatternField>,
    },
    /// A qualified or tuple-constructor variant pattern.
    EnumVariant {
        /// Unresolved variant path.
        path: String,
        /// Payload patterns; empty for a payload-free variant.
        fields: Vec<PatternId>,
    },
}

/// An inline range-pattern endpoint before constant evaluation.
///
/// `1..=LIMIT` stores `start: Literal(Literal { kind: Number, text: "1" })`
/// and `end: Path("LIMIT")` in PatternKind::Range. A qualified constant retains
/// the whole path string; missing recovered endpoints can use `"<missing>"`.
/// These are spellings, not computed scalar values or expression-table links;
/// checking resolves/evaluates them under pattern constraints.
#[derive(Debug, Clone)]
pub enum PatternBound {
    /// A literal endpoint, such as `1`.
    Literal(Literal),
    /// A constant path; recovery may produce `<missing>`.
    Path(String),
}

/// A named struct-pattern member stored inline in PatternKind::Struct.fields.
///
/// ```text
/// Point { x: n, y }
/// PatternField { name: "x", pattern: n_pattern }
/// PatternField { name: "y", pattern: y_pattern }
/// n_pattern -> Body.pattern -> Name { name: "n", local: n_local }
/// y_pattern -> Body.pattern -> Name { name: "y", local: y_local } // synthesized shorthand
/// ```
///
/// `name` selects the matched object's field; the nested pattern can bind a
/// DIFFERENT name, as x/n demonstrates. This record has no FieldId; checking
/// selects the declaration from the matched type and validates the subpattern.
#[derive(Debug, Clone)]
pub struct PatternField {
    /// Field spelling to resolve against the matched struct.
    pub name: String,
    /// Nested pattern applied to the selected field.
    pub pattern: PatternId,
}
