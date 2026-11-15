//! Cursor projections borrowing one fixed full-file semantic result.

use kagari_common::identity::table::DefinitionId;
use kagari_hir::{
    analysis::FileAnalysis,
    declarations::{Declaration, DeclarationId, Declarations},
    hir::{expr::ExprKind, place::PlaceKind},
    imports::functions::ImportedFunction,
    lower::LoweredModule,
    resolver::resolved::ResolvedName,
    typeck::table::{CallTarget, ResolvedTypeTarget, TypeTable},
    types::TypeId,
};

use crate::{declarations::DeclarationQueries, scope::ScopeIndex, sites::SourceSites};
use kagari_common::cancellation::CancellationToken;

/// A visible lexical declaration paired with its checked type in this snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisibleBinding {
    /// Type whose definition IDs belong to the originating file analysis snapshot.
    pub ty: TypeId,
    /// Binding declaration whose scoped IDs belong to the originating file analysis.
    pub declaration: Declaration,
}

/// Editor queries over a matching full-file semantic owner.
///
/// Offsets are UTF-8 byte offsets in the retained source, including CRLF bytes.
/// Returned types and declaration IDs use that result's definition table. Recovery
/// facts remain queryable after source errors without conferring compilation authority.
#[derive(Debug, Clone, Copy)]
pub struct FileQueries<'a> {
    pub(crate) analysis: &'a FileAnalysis,
}

impl<'a> FileQueries<'a> {
    /// Borrows an immutable semantic result; no analysis or indexes are prepared.
    pub fn new(analysis: &'a FileAnalysis) -> Self {
        Self { analysis }
    }

    /// Return the checked field identity selected at a member name.
    ///
    /// Covers source and external fields in reads and assignment paths. Receiver
    /// and punctuation positions have no field target. An external schema can
    /// retain an identity without a source declaration location; resolve it through
    /// this snapshot's definition table and the owning shared type schema.
    pub fn field_at(&self, offset: usize) -> Option<DefinitionId> {
        let facts = self.analysis.result().facts();
        let sites = SourceSites::build(
            &facts.lowered,
            &self.analysis.syntax(),
            &CancellationToken::default(),
        )
        .expect("uncancelled source sites");
        let expressions = facts
            .lowered
            .module
            .body
            .expressions()
            .filter_map(|(id, expr)| {
                let ExprKind::Field { .. } = &expr.kind else {
                    return None;
                };
                let field = facts.typed.type_table().expr_field(id)?;
                let span = sites.expr_reference_span(id)?;
                (span.start <= offset && offset < span.end).then_some(*field)
            });
        let places = facts
            .lowered
            .module
            .body
            .places()
            .filter_map(|(id, place)| {
                let PlaceKind::Field { .. } = &place.kind else {
                    return None;
                };
                let field = facts.typed.type_table().place_field(id)?;
                let span = sites.place_member_span(id)?;
                (span.start <= offset && offset < span.end).then_some(*field)
            });
        expressions.chain(places).next()
    }

    /// Finds an imported source function at a reference, checked call or import directive.
    pub fn source_function_at(&self, offset: usize) -> Option<&'a ImportedFunction> {
        let facts = self.analysis.result().facts();
        let sites = SourceSites::build(
            &facts.lowered,
            &self.analysis.syntax(),
            &CancellationToken::default(),
        )
        .expect("uncancelled source sites");
        facts
            .lowered
            .module
            .body
            .expressions()
            .filter_map(|(id, expr)| {
                let span = facts.lowered.source_map.expr_span(id);
                let span = match &expr.kind {
                    ExprKind::Field { .. } | ExprKind::Name { .. } => {
                        sites.expr_reference_span(id)?
                    }
                    _ => span,
                };
                if !(span.start <= offset && offset < span.end) {
                    return None;
                }
                let function = facts
                    .imported_functions
                    .get(facts.names.expr_resolution(id)?)?;
                Some((span.end - span.start, function))
            })
            .min_by_key(|(length, _)| *length)
            .map(|(_, function)| function)
            .or_else(|| {
                facts
                    .lowered
                    .module
                    .body
                    .expressions()
                    .filter_map(|(id, expr)| {
                        let ExprKind::Call { callee, .. } = &expr.kind else {
                            return None;
                        };
                        let CallTarget::SourceFunction(function) =
                            facts.typed.type_table().call_resolution(id)?.target
                        else {
                            return None;
                        };
                        let span = sites.expr_reference_span(*callee)?;
                        (span.start <= offset && offset < span.end)
                            .then(|| facts.imported_functions.target(&function))
                            .flatten()
                    })
                    .next()
            })
            .or_else(|| {
                facts.names.imports.directives.iter().find_map(|import| {
                    (import.span.range.start <= offset && offset < import.span.range.end)
                        .then(|| {
                            facts.imported_functions.get(
                                import
                                    .resolution
                                    .target()?
                                    .resolved(facts.names.items.source_unit()),
                            )
                        })
                        .flatten()
                })
            })
    }

    /// Returns the most specific available checked type at a source byte offset.
    pub fn type_at(&self, offset: usize) -> Option<TypeId> {
        let facts = self.analysis.result().facts();
        let ty = type_at_in(&facts.lowered, facts.typed.type_table(), offset)?;
        Some(ty)
    }

    /// Returns the checked receiver type for member access at this byte offset.
    pub fn member_receiver_type(&self, offset: usize) -> Option<TypeId> {
        let facts = self.analysis.result().facts();
        let ty = member_receiver_type_in(&facts.lowered, facts.typed.type_table(), offset)?;
        Some(ty)
    }

    /// Finds a declaration site or the declaration selected by a checked source reference.
    pub fn definition_at(&self, offset: usize) -> Option<&'a Declaration> {
        let facts = self.analysis.result().facts();
        if let Some(declaration) =
            DeclarationQueries::new(&facts.lowered, &facts.declarations).site_at(offset)
        {
            return Some(declaration);
        }
        if let Some(member) = facts
            .aggregates
            .traits()
            .flat_map(|contract| contract.associated_consts.values())
            .find(|member| {
                member.declaration.location.file == self.analysis.source().id()
                    && member.declaration.location.range.start <= offset
                    && offset < member.declaration.location.range.end
            })
        {
            return Some(&member.declaration);
        }
        let sites = SourceSites::build(
            &facts.lowered,
            &self.analysis.syntax(),
            &CancellationToken::default(),
        )
        .expect("uncancelled source sites");
        let declaration = |target: ResolvedName| {
            if let ResolvedName::Source(source) = &target
                && let Some(item) = facts.imported_statics.get(source)
            {
                return Some(&item.site);
            }
            if let ResolvedName::Source(source) = &target
                && let Some(item) = facts.imported_constants.get(source)
            {
                return Some(&item.site);
            }
            facts.declarations.target(target)
        };
        let expressions = facts
            .lowered
            .module
            .body
            .expressions()
            .filter_map(|(id, expr)| {
                let span = facts.lowered.source_map.expr_span(id);
                let span = match &expr.kind {
                    ExprKind::Field { .. }
                    | ExprKind::Name { .. }
                    | ExprKind::StructInit { .. } => sites.expr_reference_span(id)?,
                    _ => span,
                };
                let target = facts
                    .names
                    .expr_resolution(id)
                    .and_then(declaration)
                    .or_else(|| {
                        let fact = facts.typed.type_table().associated_const(id)?;
                        Some(
                            &facts
                                .aggregates
                                .trait_(&fact.interface.declaration)?
                                .associated_consts
                                .get(&fact.member)?
                                .declaration,
                        )
                    })
                    .or_else(|| {
                        facts.typed.type_table().expr_field(id).and_then(|field| {
                            facts
                                .aggregates
                                .field(field)
                                .map(|field| &field.declaration)
                        })
                    })
                    .or_else(|| {
                        facts
                            .typed
                            .type_table()
                            .enum_constructor(id)
                            .filter(|_| {
                                matches!(facts.lowered.module.expr(id).kind, ExprKind::Name { .. })
                            })
                            .and_then(|target| target.variant.as_ref())
                            .and_then(|variant| facts.aggregates.variant(variant))
                            .map(|variant| &variant.declaration)
                    })
                    .or_else(|| {
                        facts
                            .typed
                            .type_table()
                            .struct_init(id)
                            .and_then(|target| facts.aggregates.structure(&target.structure))
                            .map(|structure| &structure.declaration)
                    })?;
                Some((span, target))
            });
        let places = facts.lowered.module.body.places().filter_map(|(id, _)| {
            let span = facts.lowered.source_map.place_span(id);
            let target = facts
                .names
                .place_resolution(id)
                .and_then(declaration)
                .or_else(|| {
                    facts.typed.type_table().place_field(id).and_then(|field| {
                        facts
                            .aggregates
                            .field(field)
                            .map(|field| &field.declaration)
                    })
                })?;
            let span = match &facts.lowered.module.place(id).kind {
                PlaceKind::Field { .. } => sites.place_member_span(id)?,
                _ => span,
            };
            Some((span, target))
        });
        let initializer_fields =
            facts
                .lowered
                .module
                .body
                .expressions()
                .filter_map(|(id, expr)| {
                    let ExprKind::StructInit { .. } = &expr.kind else {
                        return None;
                    };
                    let spans = sites.struct_field_spans(id)?;
                    let resolved = facts.typed.type_table().struct_init(id)?;
                    spans
                        .iter()
                        .zip(&resolved.fields)
                        .filter_map(|(span, field)| {
                            let span = (*span)?;
                            let field = facts.aggregates.field(field.as_ref()?)?;
                            (span.start <= offset && offset < span.end)
                                .then_some((span, &field.declaration))
                        })
                        .next()
                });
        let enum_owners = facts
            .lowered
            .module
            .body
            .expressions()
            .filter_map(|(id, expr)| {
                let ExprKind::Name { .. } = &expr.kind else {
                    return None;
                };
                let span = sites.expr_owner_span(id)?;
                if !(span.start <= offset && offset < span.end) {
                    return None;
                }
                let constructor = facts.typed.type_table().enum_constructor(id)?;
                let enumeration = facts.aggregates.enumeration(&constructor.enumeration)?;
                Some((span, &enumeration.declaration))
            });
        let calls = facts
            .lowered
            .module
            .body
            .expressions()
            .filter_map(|(id, expr)| {
                let ExprKind::Call { callee, .. } = &expr.kind else {
                    return None;
                };
                let call = facts.typed.type_table().call_resolution(id)?;
                let callee_span = facts.lowered.source_map.expr_span(*callee);
                let callee_span = match &facts.lowered.module.expr(*callee).kind {
                    ExprKind::Field { .. } | ExprKind::Name { .. } => {
                        sites.expr_reference_span(*callee)?
                    }
                    _ => callee_span,
                };
                match call.target {
                    CallTarget::Function(function) => Some((
                        callee_span,
                        facts
                            .declarations
                            .target(ResolvedName::Function(function))?,
                    )),
                    CallTarget::SourceFunction(function) => Some((
                        callee_span,
                        &facts.imported_functions.target(&function)?.site,
                    )),
                    CallTarget::TraitMethod { method, .. } => Some((
                        callee_span,
                        &facts.aggregates.trait_method(&method)?.declaration,
                    )),
                    _ => None,
                }
            });
        let patterns = facts.lowered.module.body.patterns().filter_map(|(id, _)| {
            let span = sites.pattern_reference_span(id)?;
            let variant = facts.typed.type_table().pattern_variant(id)?;
            Some((span, &facts.aggregates.variant(variant)?.declaration))
        });
        expressions
            .chain(patterns)
            .chain(places)
            .chain(initializer_fields)
            .chain(enum_owners)
            .chain(calls)
            .filter(|(span, _)| span.start <= offset && offset < span.end)
            .min_by_key(|(span, _)| span.end - span.start)
            .map(|(_, target)| target)
            .or_else(|| {
                type_reference_at(
                    &facts.lowered,
                    &sites,
                    facts.typed.type_table(),
                    &facts.declarations,
                    offset,
                )
                .flatten()
            })
    }

    /// Collects visible lexical bindings and their checked types at a source byte offset.
    pub fn visible_bindings(&self, offset: usize) -> Vec<VisibleBinding> {
        let facts = self.analysis.result().facts();
        ScopeIndex::new(&facts.lowered, &facts.names)
            .visible_bindings(offset)
            .into_iter()
            .filter_map(|binding| {
                let declaration = facts.declarations.target(binding.resolved.clone())?.clone();
                let ty = match binding.resolved.clone() {
                    ResolvedName::Local(id) => facts.typed.type_table().local_type(id),
                    ResolvedName::Param(id) => facts
                        .typed
                        .signatures()
                        .parameter(id)
                        .map(|param| param.ty.clone()),
                    _ => None,
                }
                .unwrap_or(TypeId::Unknown);
                Some(VisibleBinding { declaration, ty })
            })
            .collect()
    }
}

/// An enclosing annotation claims the position even when its terminal name is
/// unresolved. This prevents an outer expression or type application target
/// from leaking into a missing nested type reference.
pub(crate) fn type_reference_at<'a>(
    lowered: &LoweredModule,
    sites: &SourceSites<'_>,
    table: &TypeTable,
    declarations: &'a Declarations,
    offset: usize,
) -> Option<Option<&'a Declaration>> {
    type_reference_target_at(lowered, sites, table, offset).map(|target| {
        target.and_then(|target| match target {
            ResolvedTypeTarget::OpaqueType(id) => declarations.target(ResolvedName::OpaqueType(id)),
            ResolvedTypeTarget::Source(id) => declarations
                .get(&DeclarationId::Definition(id))
                .or_else(|| {
                    declarations
                        .imported_types()
                        .by_declaration(&id)
                        .map(|ty| &ty.declaration)
                }),
            ResolvedTypeTarget::Struct(id) => declarations.target(ResolvedName::Struct(id)),
            ResolvedTypeTarget::Enum(id) => declarations.target(ResolvedName::Enum(id)),
            ResolvedTypeTarget::Trait(id) => declarations.target(ResolvedName::Trait(id)),
            ResolvedTypeTarget::Generic(id) => declarations.generic_parameter(id),
            ResolvedTypeTarget::AssociatedType(id) => {
                declarations.get(&DeclarationId::Definition(id))
            }
        })
    })
}

pub(crate) fn type_reference_target_at(
    lowered: &LoweredModule,
    sites: &SourceSites<'_>,
    table: &TypeTable,
    offset: usize,
) -> Option<Option<ResolvedTypeTarget>> {
    let (type_id, _) = lowered
        .module
        .body
        .types()
        .map(|(id, _)| (id, lowered.source_map.type_span(id)))
        .filter(|(_, span)| span.start <= offset && offset < span.end)
        .min_by_key(|(_, span)| span.end - span.start)?;
    let resolved = table.type_ref(type_id)?;

    let name_span = if matches!(resolved.target, Some(ResolvedTypeTarget::AssociatedType(_)))
        || matches!(resolved.ty, TypeId::Host(_))
    {
        sites.type_terminal_span(type_id)
    } else {
        sites.type_name_span(type_id)
    };
    let target = name_span
        .filter(|span| span.start <= offset && offset < span.end)
        .and_then(|_| resolved.target.clone());
    Some(target)
}

pub(crate) fn type_at_in(
    lowered: &LoweredModule,
    table: &TypeTable,
    offset: usize,
) -> Option<TypeId> {
    let expressions = lowered.module.body.expressions().filter_map(|(id, _)| {
        let span = lowered.source_map.expr_span(id);
        (span.start <= offset && offset < span.end)
            .then(|| table.expr_type(id).map(|ty| (span.end - span.start, ty)))
            .flatten()
    });
    let types = lowered.module.body.types().filter_map(|(id, _)| {
        let span = lowered.source_map.type_span(id);
        if !(span.start <= offset && offset < span.end) {
            return None;
        }
        table
            .type_ref(id)
            .map(|resolved| (span.end - span.start, resolved.ty.clone()))
    });
    let places = lowered.module.body.places().filter_map(|(id, _)| {
        let span = lowered.source_map.place_span(id);
        (span.start <= offset && offset < span.end)
            .then(|| table.place_type(id).map(|ty| (span.end - span.start, ty)))
            .flatten()
    });
    expressions
        .chain(types)
        .chain(places)
        .min_by_key(|(len, _)| *len)
        .map(|(_, ty)| ty)
}

pub(crate) fn member_receiver_type_in(
    lowered: &LoweredModule,
    table: &TypeTable,
    offset: usize,
) -> Option<TypeId> {
    let expressions = lowered.module.body.expressions().filter_map(|(id, expr)| {
        let ExprKind::Field { receiver, .. } = &expr.kind else {
            return None;
        };
        let span = lowered.source_map.expr_span(id);
        if span.start <= offset && offset <= span.end {
            table
                .expr_type(*receiver)
                .map(|ty| (span.end - span.start, ty))
        } else {
            None
        }
    });
    let places = lowered.module.body.places().filter_map(|(id, place)| {
        let PlaceKind::Field { base, .. } = &place.kind else {
            return None;
        };
        let span = lowered.source_map.place_span(id);
        (span.start <= offset && offset <= span.end)
            .then(|| {
                table
                    .place_type(*base)
                    .map(|ty| (span.end - span.start, ty))
            })
            .flatten()
    });
    expressions
        .chain(places)
        .min_by_key(|(len, _)| *len)
        .map(|(_, ty)| ty)
}
