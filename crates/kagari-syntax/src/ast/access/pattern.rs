//! Read-only pattern classification and explicitly stored children.

use crate::ast::{
    borrow::Ast,
    expr::{Literal, PathExpr, Pattern, PatternBound, PatternField, RangeOp},
    misc::Name,
    views::PatternBoundView,
};

impl<'a> Ast<'a, Pattern> {
    pub fn is_or(self) -> bool {
        matches!(self.node, Pattern::Or(_))
    }

    pub fn is_struct(self) -> bool {
        matches!(self.node, Pattern::Struct(_))
    }

    pub fn is_tuple_struct(self) -> bool {
        matches!(self.node, Pattern::TupleStruct(_))
    }

    pub fn is_tuple(self) -> bool {
        matches!(self.node, Pattern::Tuple(_))
    }

    pub fn is_grouped(self) -> bool {
        matches!(self.node, Pattern::Grouped(_))
    }

    pub fn has_rest(self) -> bool {
        matches!(self.node, Pattern::Struct(pattern) if pattern.rest.is_some())
    }

    pub fn is_wildcard(self) -> bool {
        matches!(self.node, Pattern::Wildcard(_))
            || self
                .path()
                .and_then(|path| path.name_text())
                .is_some_and(|name| name == "_")
    }

    pub fn path(self) -> Option<Ast<'a, PathExpr>> {
        match self.node {
            Pattern::Path(path) => Some(self.with::<PathExpr>(path)),
            Pattern::TupleStruct(pattern) => Some(self.with(&pattern.path)),
            Pattern::Struct(pattern) => Some(self.with(&pattern.path)),
            _ => None,
        }
    }

    pub fn literal(self) -> Option<Ast<'a, Literal>> {
        match self.node {
            Pattern::Literal(literal) => Some(self.with(literal)),
            _ => None,
        }
    }

    pub fn elements(self) -> impl Iterator<Item = Ast<'a, Pattern>> {
        let (first, entries, alternatives) = match self.node {
            Pattern::Or(pattern) => (
                pattern.first.as_ref(),
                &[][..],
                pattern.alternatives.as_slice(),
            ),
            Pattern::Grouped(pattern) => (pattern.pattern.as_ref(), &[][..], &[][..]),
            Pattern::Tuple(pattern) => (None, pattern.elements.entries.as_ref(), &[][..]),
            Pattern::TupleStruct(pattern) => (None, pattern.elements.entries.as_ref(), &[][..]),
            _ => (None, &[][..], &[][..]),
        };
        first
            .into_iter()
            .chain(entries.iter().filter_map(|entry| entry.element.as_ref()))
            .chain(
                alternatives
                    .iter()
                    .filter_map(|alternative| alternative.pattern.as_ref()),
            )
            .map(move |pattern| self.with(pattern))
    }

    pub fn fields(self) -> impl Iterator<Item = Ast<'a, PatternField>> {
        let entries = match self.node {
            Pattern::Struct(pattern) => pattern.fields.entries.as_ref(),
            _ => &[],
        };
        entries
            .iter()
            .filter_map(move |entry| entry.element.as_ref().map(|field| self.with(field)))
    }

    pub fn range_inclusive(self) -> Option<bool> {
        match self.node {
            Pattern::Range(range) => Some(range.operator.operation == RangeOp::Inclusive),
            _ => None,
        }
    }

    pub fn range_bounds(self) -> impl Iterator<Item = PatternBoundView<'a>> {
        let bounds = match self.node {
            Pattern::Range(range) => Some([&range.start, &range.end]),
            _ => None,
        };
        bounds.into_iter().flatten().filter_map(move |bound| {
            bound.as_ref().map(|bound| match bound {
                PatternBound::Literal(literal) => PatternBoundView::Literal(self.with(literal)),
                PatternBound::Path(path) => PatternBoundView::Path(self.with(path)),
            })
        })
    }
}

impl<'a> Ast<'a, PatternField> {
    required_child!(name, Name, name);

    pub fn pattern(self) -> Option<Ast<'a, Pattern>> {
        self.node
            .pattern
            .as_ref()?
            .pattern
            .as_ref()
            .map(|pattern| self.with(pattern))
    }
}
