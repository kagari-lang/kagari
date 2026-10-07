//! Match and binding pattern storage, including provisional local identities.

use crate::hir::{
    expr::literal::Literal,
    ids::{LocalId, PatternId},
};

/// A match/binding pattern stored by `PatternId` in the shared body arena.
///
/// ```text
/// (x, _)
/// PatternKind::Tuple([p0, p1])
/// +-- p0 -> Name { name: "x", local: l }   // l keys resolution/type facts
/// `-- p1 -> Wildcard
///
/// Point { x } -> Struct { path: "Point", fields: [PatternField { name: "x", pattern: p0 }] }
/// Choice::Some(x) -> EnumVariant { path: "Choice::Some", fields: [p0] }
/// ```
///
/// Fields/alternatives retain source order. Grouping parentheses collapse to their
/// inner pattern. A bare identifier is initially a `Name`; resolution can distinguish
/// a binding from a visible enum variant. Recovery may use a `<missing>` name.
#[derive(Debug, Clone)]
pub struct PatternData {
    /// Pattern structure and binding IDs, before constructor/type resolution.
    pub kind: PatternKind,
}

/// Structural pattern forms; nested patterns are stored by ID.
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

/// A range-pattern endpoint retained before constant evaluation.
#[derive(Debug, Clone)]
pub enum PatternBound {
    /// A literal endpoint, such as `1`.
    Literal(Literal),
    /// A constant path; recovery may produce `<missing>`.
    Path(String),
}

/// One named struct-pattern field; shorthand creates a nested name pattern.
#[derive(Debug, Clone)]
pub struct PatternField {
    /// Field spelling to resolve against the matched struct.
    pub name: String,
    /// Nested pattern applied to the selected field.
    pub pattern: PatternId,
}
