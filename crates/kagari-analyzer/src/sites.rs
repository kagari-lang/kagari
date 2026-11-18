//! Query-local reference sites projected from the exact retained syntax and HIR origins.

use kagari_common::{
    cancellation::{CancellationToken, Cancelled},
    span::Span,
};
use kagari_hir::{
    hir::ids::{ExprId, PatternId, PlaceId, TypeRefId},
    lower::LoweredModule,
};
use kagari_syntax::ast::{
    borrow::Ast,
    item::UseTree,
    misc::{Name, Path},
    module::ModuleSyntax,
    views::TypeView,
    visit::NodeRef,
};
use std::collections::HashMap;

#[derive(Debug, Default)]
struct ExpressionSite {
    reference: Option<Span>,
    owner: Option<Span>,
    fields: Vec<Option<Span>>,
    path: Vec<(String, Span)>,
}

#[derive(Debug, Default)]
struct TypeSite {
    name: Option<Span>,
    terminal: Option<Span>,
    path: Vec<(String, Span)>,
}

/// Coordinates always address the analysis text, never a copied declaration's location.
/// The borrowed lowering checks node identity/ownership before mapping its whole-node span.
#[derive(Debug)]
pub(crate) struct SourceSites<'a> {
    lowered: &'a LoweredModule,
    expressions: HashMap<(usize, usize), ExpressionSite>,
    types: HashMap<(usize, usize), TypeSite>,
    patterns: HashMap<(usize, usize), Span>,
    pub(crate) imports: Vec<Vec<(usize, Span)>>,
}

fn key(span: Span) -> (usize, usize) {
    (span.start, span.end)
}

fn segments<'a>(names: impl Iterator<Item = Ast<'a, Name>>) -> Vec<(String, Span)> {
    names
        .filter_map(|name| Some((name.text()?, name.body_span())))
        .collect()
}

fn type_site(path: Option<Ast<'_, Path>>, terminal: Option<Ast<'_, Name>>) -> TypeSite {
    TypeSite {
        name: path
            .map(|path| path.body_span())
            .or_else(|| terminal.map(|name| name.body_span())),
        terminal: path
            .and_then(|path| path.segments().last())
            .or(terminal)
            .map(|name| name.body_span()),
        path: path
            .map(|path| segments(path.segments()))
            .unwrap_or_default(),
    }
}

impl<'a> SourceSites<'a> {
    pub(crate) fn build(
        lowered: &'a LoweredModule,
        syntax: &Option<ModuleSyntax>,
        cancel: &CancellationToken,
    ) -> Result<Self, Cancelled> {
        cancel.check()?;
        let mut sites = Self {
            lowered,
            expressions: HashMap::new(),
            types: HashMap::new(),
            patterns: HashMap::new(),
            imports: Vec::new(),
        };
        let Some(syntax) = syntax else {
            return Ok(sites);
        };
        syntax.visit_owned(&mut |node| {
            if cancel.check().is_err() {
                return false;
            }
            match node {
                NodeRef::FieldExpr(field) => {
                    sites.expressions.insert(
                        key(field.body_span()),
                        ExpressionSite {
                            reference: field.name().map(|name| name.body_span()),
                            ..Default::default()
                        },
                    );
                }
                NodeRef::PathExpr(path) => {
                    let path_segments = path
                        .path()
                        .map(|path| path.segments().collect::<Vec<_>>())
                        .unwrap_or_default();
                    let owner = path_segments
                        .get(
                            path_segments
                                .len()
                                .saturating_sub(if path.name().is_some() { 1 } else { 2 }),
                        )
                        .filter(|_| path.name().is_some() || path_segments.len() >= 2)
                        .map(|name| name.body_span());
                    sites.expressions.insert(
                        key(path.body_span()),
                        ExpressionSite {
                            reference: path
                                .name()
                                .or_else(|| path.path()?.segments().last())
                                .or_else(|| path.qualified_type()?.member())
                                .map(|name| name.body_span()),
                            owner,
                            path: segments(path_segments.into_iter().chain(path.name())),
                            ..Default::default()
                        },
                    );
                    if let Some(arguments) = path.generic_args() {
                        let mut span = arguments.body_span();
                        if let Some(base) = path.path() {
                            span.start = base.body_span().start;
                        }
                        sites.types.insert(
                            key(span),
                            TypeSite {
                                name: path
                                    .path()
                                    .and_then(|path| path.segments().last())
                                    .map(|name| name.body_span()),
                                ..Default::default()
                            },
                        );
                    }
                }
                NodeRef::StructExpr(expr) => {
                    sites.expressions.insert(
                        key(expr.body_span()),
                        ExpressionSite {
                            reference: expr
                                .path()
                                .and_then(|path| {
                                    path.name().or_else(|| path.path()?.segments().last())
                                })
                                .map(|name| name.body_span()),
                            fields: expr
                                .field_list()
                                .fields()
                                .map(|field| field.name().map(|name| name.body_span()))
                                .collect(),
                            ..Default::default()
                        },
                    );
                    if let Some(arguments) = expr.generic_args() {
                        let mut span = arguments.body_span();
                        if let Some(path) = expr.path() {
                            span.start = path.body_span().start;
                        }
                        sites.types.insert(
                            key(span),
                            TypeSite {
                                name: expr
                                    .path()
                                    .and_then(|path| path.path()?.segments().last())
                                    .map(|name| name.body_span()),
                                ..Default::default()
                            },
                        );
                    }
                }
                NodeRef::FieldInit(field) if field.value().is_none() => {
                    if let Some(name) = field.name() {
                        sites.expressions.insert(
                            key(name.body_span()),
                            ExpressionSite {
                                reference: Some(name.body_span()),
                                ..Default::default()
                            },
                        );
                    }
                }
                NodeRef::Pattern(pattern) => {
                    if let Some(name) = pattern
                        .path()
                        .and_then(|path| path.name().or_else(|| path.path()?.segments().last()))
                    {
                        sites
                            .patterns
                            .insert(key(pattern.body_span()), name.body_span());
                    }
                }
                NodeRef::Type(ty) => {
                    let site = match ty.view() {
                        TypeView::Named(path) => type_site(Some(path), None),
                        TypeView::Applied(applied) => type_site(applied.path(), None),
                        TypeView::Qualified(qualified) => type_site(None, qualified.member()),
                        _ => return true,
                    };
                    sites.types.insert(key(ty.body_span()), site);
                }
                NodeRef::QualifiedType(ty) => {
                    sites
                        .types
                        .insert(key(ty.body_span()), type_site(None, ty.member()));
                }
                NodeRef::TraitRef(reference) => {
                    sites.types.insert(
                        key(reference.body_span()),
                        TypeSite {
                            name: reference
                                .path()
                                .and_then(|path| path.segments().last())
                                .map(|name| name.body_span()),
                            ..Default::default()
                        },
                    );
                }
                NodeRef::UseDecl(declaration) => {
                    if let Some(tree) = declaration.tree() {
                        sites.import_tree(&tree, Vec::new(), cancel);
                    }
                }
                _ => {}
            }
            true
        });
        cancel.check()?;
        Ok(sites)
    }

    fn import_tree(
        &mut self,
        tree: &Ast<'_, UseTree>,
        mut base: Vec<(usize, Span)>,
        cancel: &CancellationToken,
    ) {
        if cancel.check().is_err() {
            return;
        }
        if let Some(path) = tree.path() {
            for segment in path.segments() {
                if segment.text().is_some() {
                    base.push((base.len() + 1, segment.body_span()));
                }
            }
        }
        let children = tree.nested_trees().collect::<Vec<_>>();
        if children.is_empty() {
            self.imports.push(base);
        } else {
            for child in children {
                self.import_tree(&child, base.clone(), cancel);
            }
        }
    }

    pub(crate) fn expr_reference_span(&self, id: ExprId) -> Option<Span> {
        self.expressions
            .get(&key(self.lowered.module.expr(id).origin.source_span()?))?
            .reference
    }

    pub(crate) fn expr_owner_span(&self, id: ExprId) -> Option<Span> {
        self.expressions
            .get(&key(self.lowered.module.expr(id).origin.source_span()?))?
            .owner
    }

    pub(crate) fn expr_path(&self, id: ExprId) -> &[(String, Span)] {
        let Some(span) = self.lowered.module.expr(id).origin.source_span() else {
            return &[];
        };
        self.expressions
            .get(&key(span))
            .map_or(&[], |site| &site.path)
    }

    pub(crate) fn struct_field_spans(&self, id: ExprId) -> Option<&[Option<Span>]> {
        Some(
            &self
                .expressions
                .get(&key(self.lowered.module.expr(id).origin.source_span()?))?
                .fields,
        )
    }

    pub(crate) fn place_member_span(&self, id: PlaceId) -> Option<Span> {
        self.expressions
            .get(&key(self.lowered.module.place(id).origin.source_span()?))?
            .reference
    }

    pub(crate) fn pattern_reference_span(&self, id: PatternId) -> Option<Span> {
        self.patterns
            .get(&key(self
                .lowered
                .module
                .pattern(id)
                .origin
                .source_span()?))
            .copied()
    }

    pub(crate) fn type_name_span(&self, id: TypeRefId) -> Option<Span> {
        self.types
            .get(&key(self
                .lowered
                .module
                .type_ref(id)
                .origin
                .source_span()?))?
            .name
    }

    pub(crate) fn type_terminal_span(&self, id: TypeRefId) -> Option<Span> {
        self.types
            .get(&key(self
                .lowered
                .module
                .type_ref(id)
                .origin
                .source_span()?))?
            .terminal
    }

    pub(crate) fn type_path(&self, id: TypeRefId) -> &[(String, Span)] {
        let Some(span) = self.lowered.module.type_ref(id).origin.source_span() else {
            return &[];
        };
        self.types.get(&key(span)).map_or(&[], |site| &site.path)
    }
}
