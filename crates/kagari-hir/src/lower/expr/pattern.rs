//! Pattern construction, shorthand binding expansion and qualified variant spelling.

use crate::{
    hir::{
        ids::PatternId,
        pattern::{PatternBound, PatternField, PatternKind, PatternLiteral, PositionalPatterns},
    },
    lower::context::{Lowerer, syntax_span, token_span},
};
use kagari_syntax::ast::{
    borrow::Ast,
    expr::{self, Pattern},
    views::PatternBoundView,
};

impl Lowerer {
    /// Collapses grouping, allocates nested patterns/local IDs and preserves constructor/name sites for later resolution.
    pub(crate) fn lower_pattern(&mut self, pattern: &Ast<'_, Pattern>) -> PatternId {
        if pattern.is_grouped()
            && let Some(inner) = pattern.elements().next()
        {
            return self.lower_pattern(&inner);
        }
        let span = syntax_span(pattern);
        let kind = if pattern.is_or() {
            PatternKind::Or(
                pattern
                    .elements()
                    .map(|element| self.lower_pattern(&element))
                    .collect(),
            )
        } else if let Some(inclusive) = pattern.range_inclusive() {
            let mut bounds = pattern.range_bounds().map(|bound| match bound {
                PatternBoundView::Literal(literal) => {
                    PatternBound::Literal(self.lower_pattern_literal(literal))
                }
                PatternBoundView::Path(path) => {
                    PatternBound::Path(path.name_text().unwrap_or_default())
                }
            });
            PatternKind::Range {
                start: bounds
                    .next()
                    .unwrap_or_else(|| PatternBound::Path("<missing>".into())),
                end: bounds
                    .next()
                    .unwrap_or_else(|| PatternBound::Path("<missing>".into())),
                inclusive,
            }
        } else if pattern.is_wildcard() {
            PatternKind::Wildcard
        } else if pattern.is_struct() {
            PatternKind::Struct {
                path: pattern
                    .path()
                    .and_then(|path| path.name_text())
                    .unwrap_or_default(),
                fields: pattern
                    .fields()
                    .map(|field| {
                        let name = field
                            .name()
                            .and_then(|name| name.text())
                            .unwrap_or_default();
                        let pattern = if let Some(nested) = field.pattern() {
                            self.lower_pattern(&nested)
                        } else {
                            let field_span =
                                field.name().map(|name| token_span(&name)).unwrap_or(span);
                            let local = self.alloc_local(field_span, name.clone());
                            self.alloc_pattern(field_span, PatternKind::Name { local })
                        };
                        PatternField { name, pattern }
                    })
                    .collect(),
                has_rest: pattern.has_rest(),
            }
        } else if pattern.is_tuple_struct() {
            PatternKind::EnumVariant {
                path: pattern
                    .path()
                    .and_then(|path| path.name_text())
                    .unwrap_or_default(),
                fields: self.lower_positional(*pattern),
            }
        } else if pattern.is_tuple() {
            PatternKind::Tuple(self.lower_positional(*pattern))
        } else if let Some(path) = pattern.path() {
            let name = path.name_text().unwrap_or_default();
            if name.contains("::") {
                PatternKind::EnumVariant {
                    path: name,
                    fields: PositionalPatterns::default(),
                }
            } else {
                let binding_span = path
                    .name()
                    .or_else(|| path.path()?.segments().last())
                    .map(|name| token_span(&name))
                    .unwrap_or(span);
                PatternKind::Name {
                    local: self.alloc_local(binding_span, name),
                }
            }
        } else if let Some(literal) = pattern.literal() {
            PatternKind::Literal(self.lower_pattern_literal(literal))
        } else {
            PatternKind::Name {
                local: self.alloc_local(span, "<missing>".to_string()),
            }
        };

        self.alloc_pattern(span, kind)
    }

    fn lower_positional(&mut self, pattern: Ast<'_, Pattern>) -> PositionalPatterns {
        PositionalPatterns {
            rest: pattern.rest_index(),
            elements: pattern.elements().map(|p| self.lower_pattern(&p)).collect(),
        }
    }

    fn lower_pattern_literal(&mut self, literal: Ast<'_, expr::PatternLiteral>) -> PatternLiteral {
        PatternLiteral {
            value: self.lower_literal(&literal.magnitude()),
            negative: literal.is_negative(),
        }
    }
}
