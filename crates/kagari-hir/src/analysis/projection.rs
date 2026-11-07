//! Read-only body projections shared by complete-file and selected-function results.

use crate::{
    aggregates::AggregateCatalog,
    analysis::{BindingInfo, type_reference_at},
    declarations::{Declaration, Declarations},
    hir::{expr::ExprKind, place::PlaceKind},
    imports::{
        constants::ImportedConstants, functions::ImportedFunctions, statics::ImportedStatics,
    },
    lower::LoweredModule,
    resolver::resolved::{ResolvedName, ResolvedNames},
    semantic::body::ModuleFacts,
    typeck::table::CallTarget,
    types::TypeId,
};

pub(crate) struct EditorFacts<'a> {
    pub lowered: &'a LoweredModule,
    pub names: &'a ResolvedNames,
    pub declarations: &'a Declarations,
    pub typed: &'a ModuleFacts,
    pub aggregates: &'a AggregateCatalog,
    pub imported_functions: &'a ImportedFunctions,
    pub imported_statics: &'a ImportedStatics,
    pub imported_constants: &'a ImportedConstants,
}

impl<'a> EditorFacts<'a> {
    /// Finds a declaration site or the declaration selected by a checked source reference.
    pub fn definition_at(&self, offset: usize) -> Option<&'a Declaration> {
        let facts = self;
        if let Some(declaration) = facts.declarations.site_at(offset) {
            return Some(declaration);
        }
        if let Some(member) = facts
            .aggregates
            .traits()
            .flat_map(|contract| contract.associated_consts.values())
            .find(|member| {
                member.declaration.location.file == self.lowered.source.id()
                    && member.declaration.location.range.start <= offset
                    && offset < member.declaration.location.range.end
            })
        {
            return Some(&member.declaration);
        }
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
                    | ExprKind::StructInit { .. } => {
                        facts.lowered.source_map.expr_reference_span(id)?
                    }
                    _ => span,
                };
                let target = facts
                    .names
                    .expr_resolution(id)
                    .and_then(declaration)
                    .or_else(|| {
                        let fact = facts.typed.type_table.associated_const(id)?;
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
                        facts.typed.type_table.expr_field(id).and_then(|field| {
                            facts
                                .aggregates
                                .field(field)
                                .map(|field| &field.declaration)
                        })
                    })
                    .or_else(|| {
                        facts
                            .typed
                            .type_table
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
                            .type_table
                            .struct_init(id)
                            .and_then(|target| facts.aggregates.structure(&target.structure))
                            .map(|structure| &structure.declaration)
                    })?;
                Some((span, target))
            });
        let places = facts
            .lowered
            .source_map
            .place_spans()
            .iter()
            .enumerate()
            .filter_map(|(index, span)| {
                let target = facts
                    .names
                    .place_resolution(facts.lowered.source_map.place_id(index))
                    .and_then(declaration)
                    .or_else(|| {
                        facts
                            .typed
                            .type_table
                            .place_field(facts.lowered.source_map.place_id(index))
                            .and_then(|field| {
                                facts
                                    .aggregates
                                    .field(field)
                                    .map(|field| &field.declaration)
                            })
                    })?;
                let span = match &facts
                    .lowered
                    .module
                    .place(facts.lowered.source_map.place_id(index))
                    .kind
                {
                    PlaceKind::Field { .. } => facts
                        .lowered
                        .source_map
                        .place_member_span(facts.lowered.source_map.place_id(index))?,
                    _ => *span,
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
                    let spans = facts.lowered.source_map.struct_field_spans(id)?;
                    let resolved = facts.typed.type_table.struct_init(id)?;
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
                let span = facts.lowered.source_map.expr_owner_span(id)?;
                if !(span.start <= offset && offset < span.end) {
                    return None;
                }
                let constructor = facts.typed.type_table.enum_constructor(id)?;
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
                let call = facts.typed.type_table.call_resolution(id)?;
                let callee_span = facts.lowered.source_map.expr_span(*callee);
                let callee_span = match &facts.lowered.module.expr(*callee).kind {
                    ExprKind::Field { .. } | ExprKind::Name { .. } => {
                        facts.lowered.source_map.expr_reference_span(*callee)?
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
        let patterns = facts
            .lowered
            .source_map
            .pattern_spans()
            .iter()
            .enumerate()
            .filter_map(|(index, _)| {
                let id = facts.lowered.source_map.pattern_id(index);
                let span = facts.lowered.source_map.pattern_reference_span(id)?;
                let variant = facts.typed.type_table.pattern_variant(id)?;
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
                    facts.lowered,
                    &facts.typed.type_table,
                    facts.declarations,
                    offset,
                )
                .flatten()
            })
    }

    /// Collects visible lexical bindings and their checked types at a source byte offset.
    pub fn visible_bindings(&self, offset: usize) -> Vec<BindingInfo> {
        let facts = self;
        facts
            .names
            .visible_bindings(offset)
            .into_iter()
            .filter_map(|binding| {
                let declaration = facts.declarations.target(binding.resolved.clone())?.clone();
                let ty = match binding.resolved.clone() {
                    ResolvedName::Local(id) => facts.typed.type_table.local_type(id),
                    ResolvedName::Param(id) => facts
                        .typed
                        .signatures
                        .functions()
                        .iter()
                        .flat_map(|function| &function.params)
                        .find(|param| param.id == id)
                        .map(|param| param.ty.clone()),
                    _ => None,
                }
                .unwrap_or(TypeId::Unknown);
                Some(BindingInfo { declaration, ty })
            })
            .collect()
    }
}
