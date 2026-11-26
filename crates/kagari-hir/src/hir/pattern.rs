//! Match and binding pattern storage, including provisional local identities.

use crate::hir::{
    expr::literal::Literal,
    ids::{LocalId, PatternId},
    origin::NodeOrigin,
};

/// A pattern row in HirNodes.patterns, addressed by PatternId, with `kind` as payload.
///
/// ```text
/// match pair { (x, _) => x }
/// outer -> Tuple(PositionalPatterns { elements: [p0, p1], rest: None })
/// p0 -> PatternData { kind: Name { local: l } }
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
    /// Source range or structured declaration attribution.
    pub origin: NodeOrigin,
    /// Pattern structure and binding IDs, before constructor/type resolution.
    pub kind: PatternKind,
}

/// Source patterns and nested links used by declarations, match, for and binding conditions.
///
/// | Pattern source | Kind (positional lists abbreviated) |
/// | --- | --- |
/// | `_` | `Wildcard` |
/// | `1 \| 2` | `Or([one_pattern, two_pattern])` |
/// | `1..=LIMIT` | `Range { start: Literal(number_one), end: Path("LIMIT"), inclusive: true }` |
/// | `x` | `Name { local: l }` |
/// | `-42` | `Literal(PatternLiteral { value: Literal { kind: Number, text: "42" }, negative: true })` |
/// | `(x, .., z)` | `Tuple(PositionalPatterns { elements: [x_pattern, z_pattern], rest: Some(1) })` |
/// | `(x, _)` / `()` | `Tuple([x_pattern, wildcard])` / `Tuple([])` |
/// | `Point { x: n, y }` | `Struct { path: "Point", fields: [PatternField { name: "x", pattern: n_pattern }, PatternField { name: "y", pattern: y_pattern }], has_rest: false }` |
/// | `Point { x, .. }` | `Struct { path: "Point", fields: [PatternField { name: "x", pattern: x_pattern }], has_rest: true }` |
/// | `Choice::Some(x)` | `EnumVariant { path: "Choice::Some", fields: [x_pattern] }` |
/// | `Choice::None` | `EnumVariant { path: "Choice::None", fields: [] }` |
///
/// Or/Tuple/variant child IDs enter HirNodes.patterns; struct fields are inline
/// records linking to subpatterns. Range endpoints are inline PatternBound values,
/// not ExprIds, and `..` sets `inclusive: false`. Bare names receive provisional
/// LocalIds: outside ordinary declarations, a visible unit variant can later
/// resolve as a constructor instead of a binding. Declaration bare names always bind. Struct shorthand `y` creates a nested Name { local }
/// pattern. Struct rest is a presence flag, not a field or child pattern; without
/// it, checking requires every field of the nominal declaration, including private
/// fields. Semantic tables own alternative-binding consistency and constructor
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
        /// Provisional local whose record retains the written name.
        local: LocalId,
    },
    /// A literal pattern whose value/type is checked later.
    Literal(PatternLiteral),
    /// Tuple element patterns and optional rest position.
    Tuple(PositionalPatterns),
    /// A named-field pattern.
    Struct {
        /// Unresolved struct path.
        path: String,
        /// Inline field-name/subpattern pairs.
        fields: Vec<PatternField>,
        /// Whether a trailing `..` explicitly permits unlisted fields.
        has_rest: bool,
    },
    /// A qualified or tuple-constructor variant pattern.
    EnumVariant {
        /// Unresolved variant path.
        path: String,
        /// Payload patterns; empty for a payload-free variant.
        fields: PositionalPatterns,
    },
}

/// Source-ordered explicit children; in `(a, .., z)` `rest` is `Some(1)`.
#[derive(Debug, Clone, Default)]
pub struct PositionalPatterns {
    /// Child pattern IDs in written order, excluding `..`.
    pub elements: Vec<PatternId>,
    /// Number of explicit children to the left of `..`, if written.
    pub rest: Option<usize>,
}

impl PositionalPatterns {
    /// Map the written children onto a known shape. No rest means exact arity.
    pub(crate) fn slots(&self, arity: usize) -> Option<Vec<usize>> {
        let gap = arity.checked_sub(self.elements.len())?;
        if gap != 0 && self.rest.is_none() {
            return None;
        }
        Some(
            (0..self.elements.len())
                .map(|i| {
                    if self.rest.is_some_and(|rest| i >= rest) {
                        i + gap
                    } else {
                        i
                    }
                })
                .collect(),
        )
    }
}

/// Literal magnitude and source sign, checked together against the expected scalar domain.
#[derive(Debug, Clone)]
pub struct PatternLiteral {
    /// Literal token payload without the sign.
    pub value: Literal,
    /// Whether a numeric token was preceded by `-`.
    pub negative: bool,
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
    /// A literal endpoint, such as `-1`.
    Literal(PatternLiteral),
    /// A constant path; recovery may produce `<missing>`.
    Path(String),
}

/// A named struct-pattern member stored inline in PatternKind::Struct.fields.
///
/// ```text
/// Point { x: n, y }
/// PatternField { name: "x", pattern: n_pattern }
/// PatternField { name: "y", pattern: y_pattern }
/// n_pattern -> HirNodes.pattern -> Name { local: n_local }
/// y_pattern -> HirNodes.pattern -> Name { local: y_local } // synthesized shorthand
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
