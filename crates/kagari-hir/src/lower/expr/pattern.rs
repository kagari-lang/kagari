use kagari_syntax::ast;

use crate::hir::pattern::PatternBound;
use crate::hir::{PatternData, PatternId, PatternKind};
use crate::lower::context::{Lowerer, syntax_span, token_span};

impl Lowerer {
    pub(crate) fn lower_pattern(&mut self, pattern: &ast::Pattern) -> PatternId {
        if pattern.is_grouped() {
            return pattern
                .elements()
                .next()
                .map(|inner| self.lower_pattern(&inner))
                .expect("grouped pattern has one element");
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
                ast::PatternBound::Literal(literal) => {
                    PatternBound::Literal(self.lower_literal(&literal))
                }
                ast::PatternBound::Path(path) => {
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
                            let local = self.alloc_local_id(field_span);
                            self.alloc_pattern(
                                field_span,
                                PatternData {
                                    kind: PatternKind::Name {
                                        name: name.clone(),
                                        local,
                                    },
                                },
                            )
                        };
                        crate::hir::PatternField { name, pattern }
                    })
                    .collect(),
            }
        } else if pattern.is_tuple_struct() {
            PatternKind::EnumVariant {
                path: pattern
                    .path()
                    .and_then(|path| path.name_text())
                    .unwrap_or_default(),
                fields: pattern
                    .elements()
                    .map(|element| self.lower_pattern(&element))
                    .collect(),
            }
        } else if pattern.is_tuple() {
            PatternKind::Tuple(
                pattern
                    .elements()
                    .map(|element| self.lower_pattern(&element))
                    .collect(),
            )
        } else if let Some(path) = pattern.path() {
            let name = path.name_text().unwrap_or_default();
            if name.contains("::") {
                PatternKind::EnumVariant {
                    path: name,
                    fields: Vec::new(),
                }
            } else {
                let binding_span = path
                    .name()
                    .or_else(|| path.path()?.segments().last())
                    .map(|name| token_span(&name))
                    .unwrap_or(span);
                PatternKind::Name {
                    name,
                    local: self.alloc_local_id(binding_span),
                }
            }
        } else if let Some(literal) = pattern.literal() {
            PatternKind::Literal(self.lower_literal(&literal))
        } else {
            PatternKind::Name {
                name: "<missing>".to_string(),
                local: self.alloc_local_id(span),
            }
        };

        self.alloc_pattern(span, PatternData { kind })
    }
}
