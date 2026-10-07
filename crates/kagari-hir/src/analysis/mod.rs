//! Protocol-independent immutable source analysis. Queries never execute code.

#[cfg(test)]
use crate::hir::ids::StructId;
use crate::{
    AnalysisPolicy, AnalysisResult, AnalyzedModule,
    analysis::{
        body_queries::FunctionAnalysis, declaration_queries::DeclarationSnapshot,
        error::AnalysisError, signature_queries::SignatureSnapshot,
    },
    analyze_parsed,
    declarations::{Declaration, DeclarationId, Declarations},
    hir::{expr::ExprKind, place::PlaceKind},
    host::HostDeclarations,
    imports::{
        BindingOrigin, ImportKind, ModuleGraph, ResolvedTarget, SourceItem, catalog::LookupHit,
        functions::ImportedFunction,
    },
    lower::LoweredModule,
    native::render::DeclarationSource,
    resolver::resolved::ResolvedName,
    typeck::{
        ModuleSignatures,
        const_budget::ConstLimits,
        reuse::BodyReuse,
        table::{CallTarget, TypeTable, TypeTarget},
    },
    types::TypeId,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        map::{DefinitionContext, DefinitionMap},
        mapping::DefinitionMappingError,
        metadata::DefinitionMetadata,
        reference::DefinitionReference,
        table::{DefinitionId, DefinitionTable},
    },
    span::Span,
};
use kagari_source::{
    identity::{FileId, Revision},
    source::SourceFile,
    source_database::SourceSnapshot,
};
use kagari_syntax::{
    ast::item::SourceFile as AstSourceFile,
    parser::{Parse, ParseLimits},
};
use kagari_types::{
    declaration::module::{DeclarationError, ModuleDecl},
    host_interface::{
        HostFunctionDeclaration,
        type_declaration::{HostFieldDeclaration, HostTypeDeclaration},
    },
};
use std::{
    cell::{OnceCell, RefCell},
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

const DEFAULT_MAX_SEMANTIC_DIAGNOSTICS: usize = 1_000;

pub mod call_queries;
pub mod documentation_queries;
pub mod error;
pub mod method_queries;
#[cfg(test)]
mod standard_query_tests;

pub mod body_queries;
mod cache;

pub mod declaration_queries;
pub(crate) mod ownership;
pub mod signature_queries;
mod target_queries;

/// One file's parsed source and completed semantic analysis, retained by a snapshot.
///
/// `result` owns checked facts plus diagnostics and their definition table. Local HIR
/// IDs index `result().facts().lowered`; scoped definition IDs resolve through
/// [`Self::definitions`]. Position queries read these facts without rechecking bodies.
/// Offsets are UTF-8 byte offsets in [`Self::source`]; missing or inapplicable facts
/// return `None`. Type queries translate scoped identities to portable paths.
#[derive(Debug)]
pub struct FileAnalysis {
    /// Cached source ranges and namespace lookup results for type navigation.
    type_hits: Vec<(Span, LookupHit)>,
    signatures_reused: bool,
    /// Borrows the immutable source revision analyzed by this result.
    source: Arc<SourceFile>,

    parsed: Parse,
    /// Semantic facts and diagnostics paired with the table owning their definition IDs.
    result: DefinitionMetadata<AnalysisResult<AnalyzedModule<DefinitionId>>>,
}

/// A visible lexical declaration paired with its checked, portable type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingInfo {
    /// Type with portable definition paths, suitable for presentation outside the snapshot.
    pub ty: TypeId,
    /// Binding declaration whose scoped IDs belong to the originating file analysis.
    pub declaration: Declaration<DefinitionId>,
}

impl FileAnalysis {
    /// Returns the registered host field selected at an expression or assignment member name.
    pub fn host_field_at(&self, offset: usize) -> Option<&HostFieldDeclaration> {
        let facts = self.result.records().facts();
        let expressions = facts
            .lowered
            .module
            .body
            .expressions()
            .filter_map(|(id, expr)| {
                let ExprKind::Field { .. } = &expr.kind else {
                    return None;
                };
                let field = facts.typed.type_table.expr_field(id)?;
                let span = facts.lowered.source_map.expr_reference_span(id)?;
                (span.start <= offset && offset < span.end)
                    .then(|| {
                        facts
                            .names
                            .hosts
                            .field(&self.definitions().resolve(*field).ok()?.to_path())
                    })
                    .flatten()
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
                let field = facts.typed.type_table.place_field(id)?;
                let span = facts.lowered.source_map.place_member_span(id)?;
                (span.start <= offset && offset < span.end)
                    .then(|| {
                        facts
                            .names
                            .hosts
                            .field(&self.definitions().resolve(*field).ok()?.to_path())
                    })
                    .flatten()
            });
        expressions.chain(places).next()
    }

    /// Whether this result's signature query reused earlier checked facts.
    /// An unchanged file shares its existing result and this original statistic.
    pub fn signatures_reused(&self) -> bool {
        self.signatures_reused
    }

    /// Borrows the shared signature-stage result used to check this file.
    pub fn signatures(&self) -> &Arc<AnalysisResult<ModuleSignatures<DefinitionId>>> {
        &self.result.records().facts().signatures
    }

    /// Finds an imported source function at a reference, checked call or import directive.
    pub fn source_function_at(&self, offset: usize) -> Option<&ImportedFunction<DefinitionId>> {
        let facts = self.result.records().facts();
        facts
            .lowered
            .module
            .body
            .expressions()
            .filter_map(|(id, expr)| {
                let span = facts.lowered.source_map.expr_span(id);
                let span = match &expr.kind {
                    ExprKind::Field { .. } | ExprKind::Name { .. } => {
                        facts.lowered.source_map.expr_reference_span(id)?
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
                            facts.typed.type_table.call_resolution(id)?.target
                        else {
                            return None;
                        };
                        let span = facts.lowered.source_map.expr_reference_span(*callee)?;
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
                                    .resolved(facts.names.items.unit.as_ref()),
                            )
                        })
                        .flatten()
                })
            })
    }

    /// Finds the registered host function referenced at this byte offset.
    pub fn host_function_at(&self, offset: usize) -> Option<&HostFunctionDeclaration> {
        let facts = self.result.records().facts();
        facts
            .lowered
            .module
            .body
            .expressions()
            .filter_map(|(id, expr)| {
                if let ExprKind::Call { callee, .. } = expr.kind
                    && let Some(call) = facts.typed.type_table.call_resolution(id)
                    && let CallTarget::HostFunction(host) = call.target
                {
                    let span = facts.lowered.source_map.expr_span(callee);
                    let span = match &facts.lowered.module.expr(callee).kind {
                        ExprKind::Field { .. } | ExprKind::Name { .. } => {
                            facts.lowered.source_map.expr_reference_span(callee)?
                        }
                        _ => span,
                    };
                    return (span.start <= offset && offset < span.end)
                        .then_some((span.end - span.start, host));
                }
                let span = facts.lowered.source_map.expr_span(id);
                let span = match &expr.kind {
                    ExprKind::Field { .. } | ExprKind::Name { .. } => {
                        facts.lowered.source_map.expr_reference_span(id)?
                    }
                    _ => span,
                };
                if !(span.start <= offset && offset < span.end) {
                    return None;
                }
                let ResolvedName::HostFunction(host) = facts.names.expr_resolution(id)? else {
                    return None;
                };
                Some((span.end - span.start, host))
            })
            .min_by_key(|(len, _)| *len)
            .and_then(|(_, host)| facts.names.hosts.function(host))
            .or_else(|| {
                facts.names.imports.directives.iter().find_map(|import| {
                    if !(import.span.range.start <= offset && offset < import.span.range.end) {
                        return None;
                    }
                    let ResolvedName::HostFunction(host) = import
                        .resolution
                        .target()?
                        .resolved(facts.names.items.unit.as_ref())
                    else {
                        return None;
                    };
                    facts.names.hosts.function(host)
                })
            })
    }

    /// Portable host documentation has no synthetic source-file location.
    pub fn host_type_at(&self, offset: usize) -> Option<&HostTypeDeclaration> {
        let facts = self.result.records().facts();
        if let Some((index, _)) = facts
            .lowered
            .source_map
            .type_spans()
            .iter()
            .enumerate()
            .filter(|(_, span)| span.start <= offset && offset < span.end)
            .min_by_key(|(_, span)| span.end - span.start)
        {
            let id = facts.lowered.source_map.type_id(index);
            let span = facts.lowered.source_map.type_terminal_span(id)?;
            if !(span.start <= offset && offset < span.end) {
                return None;
            }
            return match &facts.typed.type_table.type_ref(id)?.target {
                Some(TypeTarget::Host(id)) => facts.names.hosts.type_declaration(*id),
                _ => None,
            };
        }
        if let Some(TypeId::Host(id)) = self.type_at(offset) {
            return facts
                .names
                .hosts
                .nominal_type(&id)
                .and_then(|id| facts.names.hosts.type_declaration(id));
        }
        facts
            .names
            .imports
            .directives
            .iter()
            .enumerate()
            .find_map(|(_index, import)| {
                if !(import.span.range.start <= offset && offset < import.span.range.end) {
                    return None;
                }
                let ResolvedName::HostType(id) = facts
                    .names
                    .imports
                    .directives
                    .get(_index)?
                    .resolution
                    .target()?
                    .resolved(facts.names.items.unit.as_ref())
                else {
                    return None;
                };
                facts.names.hosts.type_declaration(id)
            })
    }

    /// Returns a typed AST view of the retained parse tree.
    pub fn syntax(&self) -> AstSourceFile {
        self.parsed.syntax()
    }

    /// Borrows the immutable source revision analyzed by this result.
    pub fn source(&self) -> &SourceFile {
        &self.source
    }

    /// Borrows recoverable semantic facts and diagnostics, including body analysis.
    pub fn result(&self) -> &AnalysisResult<AnalyzedModule<DefinitionId>> {
        self.result.records()
    }

    /// Returns the most specific available checked type at a source byte offset.
    pub fn type_at(&self, offset: usize) -> Option<TypeId> {
        let facts = self.result.records().facts();
        let ty = type_at_in(&facts.lowered, &facts.typed.type_table, offset)?;
        ownership::paths(&ty, self.definitions(), &CancellationToken::default()).ok()
    }

    /// Returns the checked receiver type for member access at this byte offset.
    pub fn member_receiver_type(&self, offset: usize) -> Option<TypeId> {
        let facts = self.result.records().facts();
        let ty = member_receiver_type_in(&facts.lowered, &facts.typed.type_table, offset)?;
        ownership::paths(&ty, self.definitions(), &CancellationToken::default()).ok()
    }

    /// Finds a declaration site or the declaration selected by a checked source reference.
    pub fn definition_at(&self, offset: usize) -> Option<&Declaration<DefinitionId>> {
        let facts = self.result.records().facts();
        if let Some(declaration) = facts.declarations.site_at(offset) {
            return Some(declaration);
        }
        if let Some(member) = facts
            .aggregates
            .traits()
            .flat_map(|contract| contract.associated_consts.values())
            .find(|member| {
                member.declaration.location.file == self.source.id()
                    && member.declaration.location.range.start <= offset
                    && offset < member.declaration.location.range.end
            })
        {
            return Some(&member.declaration);
        }
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
                    .and_then(|target| facts.declarations.target(target))
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
                    .and_then(|target| facts.declarations.target(target))
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
                    &facts.lowered,
                    &facts.typed.type_table,
                    &facts.declarations,
                    offset,
                )
                .flatten()
            })
    }

    /// Collects visible lexical bindings and their checked types at a source byte offset.
    pub fn visible_bindings(&self, offset: usize) -> Vec<BindingInfo> {
        let facts = self.result.records().facts();
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
                        .functions
                        .iter()
                        .flat_map(|function| &function.params)
                        .find(|param| param.id == id)
                        .map(|param| param.ty.clone()),
                    _ => None,
                }
                .unwrap_or(TypeId::Unknown);
                Some(BindingInfo {
                    declaration,
                    ty: ownership::paths(&ty, self.definitions(), &CancellationToken::default())
                        .ok()?,
                })
            })
            .collect()
    }
}

/// An enclosing annotation claims the position even when its terminal name is
/// unresolved. This prevents an outer expression or type application target
/// from leaking into a missing nested type reference.
fn type_reference_at<'a, I: DefinitionReference>(
    lowered: &LoweredModule,
    table: &TypeTable<I>,
    declarations: &'a Declarations<I>,
    offset: usize,
) -> Option<Option<&'a Declaration<I>>> {
    type_reference_target_at(lowered, table, offset).map(|target| {
        target.and_then(|target| match target {
            TypeTarget::OpaqueType(id) => declarations.target(ResolvedName::OpaqueType(id)),
            TypeTarget::Host(_) => None,
            TypeTarget::Source(id) => declarations
                .imported_types()
                .by_declaration(&id)
                .map(|ty| &ty.declaration),
            TypeTarget::Struct(id) => declarations.target(ResolvedName::Struct(id)),
            TypeTarget::Enum(id) => declarations.target(ResolvedName::Enum(id)),
            TypeTarget::Trait(id) => declarations.target(ResolvedName::Trait(id)),
            TypeTarget::Generic(id) => declarations.generic_parameter(id),
            TypeTarget::AssociatedType(id) => declarations.get(&DeclarationId::Definition(id)),
        })
    })
}

fn type_reference_target_at<I: DefinitionReference>(
    lowered: &LoweredModule,
    table: &TypeTable<I>,
    offset: usize,
) -> Option<Option<TypeTarget<I>>> {
    let (index, _) = lowered
        .source_map
        .type_spans()
        .iter()
        .enumerate()
        .filter(|(_, span)| span.start <= offset && offset < span.end)
        .min_by_key(|(_, span)| span.end - span.start)?;
    let type_id = lowered.source_map.type_id(index);
    let resolved = table.type_ref(type_id)?;
    let name_span = if matches!(resolved.target, Some(TypeTarget::AssociatedType(_))) {
        lowered.source_map.type_terminal_span(type_id)
    } else {
        lowered.source_map.type_name_span(type_id)
    };
    let target = name_span
        .filter(|span| span.start <= offset && offset < span.end)
        .and_then(|_| resolved.target.clone());
    Some(target)
}

fn type_at_in<I: DefinitionReference>(
    lowered: &LoweredModule,
    table: &TypeTable<I>,
    offset: usize,
) -> Option<TypeId<I>> {
    let expressions = lowered.module.body.expressions().filter_map(|(id, _)| {
        let span = lowered.source_map.expr_span(id);
        (span.start <= offset && offset < span.end)
            .then(|| table.expr_type(id).map(|ty| (span.end - span.start, ty)))
            .flatten()
    });
    let types = lowered
        .source_map
        .type_spans()
        .iter()
        .enumerate()
        .filter_map(|(index, span)| {
            if !(span.start <= offset && offset < span.end) {
                return None;
            }
            table
                .type_ref(lowered.source_map.type_id(index))
                .map(|resolved| (span.end - span.start, resolved.ty.clone()))
        });
    let places = lowered
        .source_map
        .place_spans()
        .iter()
        .enumerate()
        .filter_map(|(index, span)| {
            (span.start <= offset && offset < span.end)
                .then(|| {
                    table
                        .place_type(lowered.source_map.place_id(index))
                        .map(|ty| (span.end - span.start, ty))
                })
                .flatten()
        });
    expressions
        .chain(types)
        .chain(places)
        .min_by_key(|(len, _)| *len)
        .map(|(_, ty)| ty)
}

fn member_receiver_type_in<I: DefinitionReference>(
    lowered: &LoweredModule,
    table: &TypeTable<I>,
    offset: usize,
) -> Option<TypeId<I>> {
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

/// Mutable owner of reusable analysis inputs and published query caches.
///
/// Choose the required depth explicitly:
///
/// ```text
/// SourceSnapshot + host/native registrations
///   declarations() -> DeclarationSnapshot: parse, lower, imports, declaration names
///   signatures()   -> SignatureSnapshot: declarations + checked signatures
///   body(owner)    -> FunctionAnalysis: signatures + selected body/constant prerequisites
///   snapshot()     -> AnalysisSnapshot: signatures + all file bodies
/// ```
///
/// The last two entrypoints check or reuse bodies before returning; snapshot position
/// queries are reads of prepared facts. Analysis errors describe cancellation or invalid
/// inputs/identity metadata; ordinary source errors are retained as diagnostics.
///
/// Caches retain immutable `Arc` results. A matching revision alone is insufficient:
/// imports, reachable namespaces, registered hosts and semantic dependencies also
/// participate in reuse. Publication checks cancellation and does not replace newer
/// published snapshots with older revisions. Earlier returned snapshots remain valid.
#[derive(Debug)]
pub struct AnalysisDatabase {
    /// Database identity context used to scope portable declaration paths.
    definitions: DefinitionContext,
    const_limits: ConstLimits,
    parse_limits: ParseLimits,
    max_semantic_diagnostics: usize,
    /// Selected-function results indexed by canonical definition identity.
    body_cache: DefinitionMap<Arc<FunctionAnalysis>>,
    /// Newest source revision admitted to the selected-body cache.
    body_revision: Revision,
    /// Latest published declaration snapshot, including shared parse/lowering results.
    declaration_cache: Option<DeclarationSnapshot>,
    /// Latest published signatures and their declaration snapshot.
    signature_cache: Option<SignatureSnapshot>,
    /// Latest fully analyzed files, retained independently of selected-body queries.
    files: HashMap<FileId, Arc<FileAnalysis>>,
    latest_revision: Revision,
    hosts: Arc<HostDeclarations>,
    /// Stable synthetic file IDs keyed by parent file and inline-module name.
    inline_ids: RefCell<HashMap<(FileId, String), FileId>>,
    native_modules: Vec<Arc<ModuleDecl>>,
    native_sources: Option<Vec<DeclarationSource>>,
    /// Parsed/lowered installed declaration sources, initialized once per registration setup.
    native_files: OnceCell<Vec<(Parse, Arc<LoweredModule>)>>,
}

impl Default for AnalysisDatabase {
    fn default() -> Self {
        Self {
            definitions: DefinitionContext::new().expect("analysis definition context exhausted"),
            parse_limits: Default::default(),
            const_limits: Default::default(),
            max_semantic_diagnostics: DEFAULT_MAX_SEMANTIC_DIAGNOSTICS,
            body_cache: DefinitionMap::default(),
            body_revision: Revision::default(),
            declaration_cache: None,
            signature_cache: None,
            files: HashMap::new(),
            latest_revision: Revision::default(),
            hosts: HostDeclarations::empty(),
            inline_ids: RefCell::new(HashMap::new()),
            native_modules: vec![],
            native_sources: None,
            native_files: OnceCell::new(),
        }
    }
}

impl AnalysisDatabase {
    /// Native declarations are explicit snapshot inputs. Existing snapshots keep their owners.
    pub fn set_native_modules(&mut self, modules: Vec<Arc<ModuleDecl>>) {
        self.native_modules = modules;
        self.native_sources = None;
        self.native_files.take();
        self.declaration_cache = None;
        self.signature_cache = None;
        self.body_cache.clear();
        self.files.clear();
    }

    /// Supply presentation origins in the same order as the explicit providers.
    /// Every source is parsed and checked against its authoritative registration.
    pub fn set_native_sources(
        &mut self,
        modules: Vec<Arc<ModuleDecl>>,
        sources: Vec<DeclarationSource>,
    ) -> Result<(), AnalysisError> {
        if modules.len() != sources.len() {
            return Err(AnalysisError::NativeApi(DeclarationError(
                "native declaration source/provider inventory differs".into(),
            )));
        }
        self.set_native_modules(modules);
        self.native_sources = Some(sources);
        Ok(())
    }

    /// Sets the per-analysis diagnostic budget, clearing body/file caches when it changes.
    pub fn set_max_semantic_diagnostics(&mut self, limit: usize) {
        if self.max_semantic_diagnostics != limit {
            self.max_semantic_diagnostics = limit;
            self.body_cache.clear();
            self.files.clear();
        }
    }

    /// Sets constant-evaluation limits, clearing body/file caches when they change.
    pub fn set_const_limits(&mut self, limits: ConstLimits) {
        if self.const_limits != limits {
            self.const_limits = limits;
            self.body_cache.clear();
            self.files.clear();
        }
    }

    /// Limits are query inputs. Existing immutable snapshots retain their facts;
    /// subsequent queries must not reuse a differently limited parse or body.
    pub fn set_parse_limits(&mut self, limits: ParseLimits) {
        if self.parse_limits == limits {
            return;
        }
        self.parse_limits = limits;
        self.native_files.take();
        self.declaration_cache = None;
        self.signature_cache = None;
        self.body_cache.clear();
        self.files.clear();
    }

    /// Replaces host inputs; later query reuse compares the host registry revision.
    pub fn set_host_declarations(&mut self, hosts: Arc<HostDeclarations>) {
        self.hosts = hosts;
    }

    /// Prepares signatures and checks or reuses bodies for every supplied/discovered file.
    ///
    /// Returns an immutable result even when source diagnostics exist. Use
    /// [`AnalysisSnapshot::check_program`] to require an error-free dependency closure.
    ///
    /// # Errors
    ///
    /// Returns [`AnalysisError`] on cancellation, invalid installed declarations or
    /// identity mapping failure. A cancelled preparation is not published as a new
    /// complete analysis snapshot.
    pub fn snapshot(
        &mut self,
        source: SourceSnapshot,
        cancel: &CancellationToken,
    ) -> Result<AnalysisSnapshot, AnalysisError> {
        cancel.check()?;
        let signature_snapshot = self.prepare_signatures(source.clone(), cancel)?;
        let graph = signature_snapshot.declarations.graph.clone();
        let signatures = signature_snapshot
            .files
            .iter()
            .map(|(id, file)| {
                (
                    *id,
                    (
                        file.prepared.lowered.source.clone(),
                        file.declaration.parsed.clone(),
                        file.prepared.clone(),
                    ),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let mut environments = signature_snapshot.body_environments(cancel)?;
        let mut files = HashMap::new();
        for (id, (file, parsed, scoped_prepared)) in signatures {
            cancel.check()?;
            let prepared = ownership::paths(
                &scoped_prepared,
                scoped_prepared.declarations.definitions(),
                cancel,
            )?;
            let previous_authoring = self
                .files
                .get(&id)
                .map(|old| old.to_unverified(cancel))
                .transpose()?;
            let environment = environments.remove(&id).expect("prepared body environment");
            let analysis = match self.files.get(&id) {
                Some(previous)
                    if previous.can_retain(
                        &prepared,
                        previous_authoring
                            .as_ref()
                            .expect("previous authoring facts")
                            .facts(),
                        &environment,
                        cancel,
                    ) =>
                {
                    previous.clone()
                }
                _ => {
                    let signatures_reused = prepared.signatures_reused;
                    let reuse = self
                        .files
                        .get(&id)
                        .filter(|old| {
                            old.can_reuse_body(
                                &prepared,
                                previous_authoring
                                    .as_ref()
                                    .expect("previous authoring facts")
                                    .facts(),
                                &environment,
                                cancel,
                            )
                        })
                        .map(|old| BodyReuse {
                            previous_diagnostics: old.result.records().diagnostics(),
                            previous_lowered: &old.result.records().facts().lowered,
                            previous_types: &previous_authoring
                                .as_ref()
                                .expect("previous authoring facts")
                                .facts()
                                .typed
                                .type_table,
                            old_text: old.source.text(),
                            new_text: file.text(),
                        });
                    let result = analyze_parsed(
                        prepared,
                        &parsed,
                        AnalysisPolicy {
                            const_limits: self.const_limits,
                            max_semantic_diagnostics: self.max_semantic_diagnostics,
                        },
                        environment.imported_functions,
                        environment.aggregates,
                        reuse.as_ref(),
                        cancel,
                    );
                    let metadata = ownership::scope(&result, &self.definitions, cancel)?;
                    let definitions = metadata.definitions().clone();
                    let mut records = metadata.into_records();
                    records
                        .facts
                        .declarations
                        .publish_definitions(definitions.clone());
                    records.facts.signatures = scoped_prepared.signatures.clone();
                    let result = DefinitionMetadata::checked(definitions, records, cancel)?;
                    let type_hits = target_queries::type_hits(result.records().facts(), cancel)?;
                    Arc::new(FileAnalysis {
                        type_hits,
                        signatures_reused,
                        source: file,
                        parsed,
                        result,
                    })
                }
            };
            files.insert(id, analysis);
        }
        cancel.check()?;
        self.publish_signatures(signature_snapshot.clone());
        if source.revision() >= self.latest_revision {
            self.latest_revision = source.revision();
            self.files = files.clone();
        }
        Ok(AnalysisSnapshot {
            definitions: self.definitions.snapshot(),
            signatures: signature_snapshot,
            revision: source.revision(),
            host_revision: self.hosts.revision(),
            graph,
            files: Arc::new(files),
        })
    }
}

/// Immutable, fully prepared analysis results from one source snapshot.
///
/// ```text
/// AnalysisSnapshot
///   signatures --shared results--> SignatureSnapshot --> DeclarationSnapshot
///   graph      --Arc-------------> ModuleGraph
///   files      --Arc<HashMap>----> FileId -> Arc<FileAnalysis>
///                                            parse + HIR + semantic side tables
///   definitions ----------------> scoped DefinitionId lookup table
/// ```
///
/// Created eagerly by [`AnalysisDatabase::snapshot`]. Position queries select the
/// appropriate inline-module analysis when necessary and read existing facts;
/// they do not execute scripts. [`Self::check_program`] validates a root's reachable
/// closure for compiler handoff. Keeping a snapshot keeps its source and facts alive
/// across later database edits.
#[derive(Debug, Clone)]
pub struct AnalysisSnapshot {
    definitions: DefinitionTable,
    signatures: SignatureSnapshot,
    /// Returns the revision of the input source snapshot.
    revision: Revision,
    /// Returns the host registry revision used by this analysis.
    host_revision: u64,
    graph: Arc<ModuleGraph>,
    files: Arc<HashMap<FileId, Arc<FileAnalysis>>>,
}

impl AnalysisSnapshot {
    /// An immutable prefix of the database's explicit identity context.
    /// Local handles additionally retain their analysis/arena ownership.
    pub fn definitions(&self) -> &DefinitionTable {
        &self.definitions
    }

    /// Select the innermost inline module at an editor position in a physical file.
    pub fn analysis_at(&self, file: FileId, offset: usize) -> Option<&Arc<FileAnalysis>> {
        let physical = self.file(file)?;
        self.files
            .values()
            .filter(|candidate| {
                let source = candidate.source();
                source.origin_id() == file
                    && source
                        .inline_range()
                        .is_some_and(|range| range.start <= offset && offset < range.end)
            })
            .min_by_key(|candidate| {
                let range = candidate.source().inline_range().expect("inline candidate");
                range.end - range.start
            })
            .or(Some(physical))
    }

    /// The declaration query consumed by this complete analysis.
    pub fn declaration_snapshot(&self) -> &DeclarationSnapshot {
        self.signatures.declaration_snapshot()
    }

    /// Borrows the signature snapshot used to prepare all file bodies.
    pub fn signature_snapshot(&self) -> &SignatureSnapshot {
        &self.signatures
    }

    /// Canonical namespace/declaration hit and the binding origins selected at this use.
    pub fn source_target_at(&self, file: FileId, offset: usize) -> Option<LookupHit> {
        let analysis = self.analysis_at(file, offset)?;
        if let Some((_, hit)) = analysis
            .type_hits
            .iter()
            .filter(|(span, _)| span.start <= offset && offset < span.end)
            .min_by_key(|(span, _)| span.end - span.start)
        {
            return Some(hit.clone());
        }
        let facts = analysis.result.records().facts();
        if let Some((_, hit)) = facts
            .names
            .path_hits
            .iter()
            .filter(|(span, _)| span.start <= offset && offset < span.end)
            .min_by_key(|(span, _)| span.end - span.start)
        {
            return Some(hit.clone());
        }
        if let Some((_, hit)) = facts
            .names
            .imports
            .path_hits
            .iter()
            .filter(|(span, _)| span.range.start <= offset && offset < span.range.end)
            .min_by_key(|(span, _)| span.range.end - span.range.start)
        {
            return Some(hit.clone());
        }
        facts
            .lowered
            .module
            .body
            .expressions()
            .filter_map(|(id, _)| {
                let span = facts
                    .lowered
                    .source_map
                    .expr_reference_span(id)
                    .unwrap_or_else(|| facts.lowered.source_map.expr_span(id));
                if !(span.start <= offset && offset < span.end) {
                    return None;
                }
                Some((span.end - span.start, facts.names.lookup_hit(id)?.clone()))
            })
            .min_by_key(|(length, _)| *length)
            .map(|(_, hit)| hit)
            .or_else(|| {
                facts.names.imports.directives.iter().find_map(|directive| {
                    if !(directive.span.range.start <= offset && offset < directive.span.range.end)
                    {
                        return None;
                    }
                    Some(LookupHit {
                        target: directive.resolution.target()?.clone(),
                        via: vec![match directive.kind {
                            ImportKind::Named { .. } => {
                                BindingOrigin::NamedImport(directive.id.clone())
                            }
                            ImportKind::Glob => BindingOrigin::GlobImport(directive.id.clone()),
                        }],
                    })
                })
            })
    }

    /// Finds a declaration at a physical source position, including inline-module routing.
    pub fn definition_at(&self, file: FileId, offset: usize) -> Option<&Declaration<DefinitionId>> {
        let analysis = self.analysis_at(file, offset)?;
        if let Some(declaration) = analysis.definition_at(offset) {
            return Some(declaration);
        }
        let facts = analysis.result.records().facts();
        if let Some(Some(TypeTarget::AssociatedType(member))) =
            type_reference_target_at(&facts.lowered, &facts.typed.type_table, offset)
        {
            return self.declaration(&DeclarationId::Definition(member));
        }
        let target = facts
            .names
            .imports
            .directives
            .iter()
            .find_map(|directive| {
                (directive.span.range.start <= offset && offset < directive.span.range.end)
                    .then(|| directive.resolution.target().cloned())
                    .flatten()
            })
            .or_else(|| self.source_target_at(file, offset).map(|hit| hit.target))?;
        let ResolvedTarget::Source(target) = target else {
            return None;
        };
        let file = self.file(target.unit.file)?;
        if !target.unit.matches(&file.result.records().facts().lowered) {
            return None;
        }
        if let SourceItem::Variant(id) = target.item {
            return file.result.records().facts().declarations.variant(id);
        }
        file.result
            .records()
            .facts()
            .declarations
            .target(target.item.local()?)
    }

    /// Borrows the import graph built for this source snapshot.
    pub fn module_graph(&self) -> &ModuleGraph {
        &self.graph
    }

    /// Returns the host registry revision used by this analysis.
    pub fn host_revision(&self) -> u64 {
        self.host_revision
    }

    /// Returns the revision of the input source snapshot.
    pub fn revision(&self) -> Revision {
        self.revision
    }

    /// Finds a physical or synthetic file analysis by its exact file ID.
    pub fn file(&self, id: FileId) -> Option<&Arc<FileAnalysis>> {
        self.files.get(&id)
    }

    /// Finds a declaration after mapping its identity into this snapshot; returns `None` if absent.
    pub fn declaration<I: DefinitionReference>(
        &self,
        id: &DeclarationId<I>,
    ) -> Option<&Declaration<DefinitionId>> {
        let id = ownership::locate(id, self.definitions())?;
        self.files
            .values()
            .find_map(|file| file.result.records().facts().declarations.get(&id))
    }

    /// Read a source owned by this analysis, including its installed standard package.
    pub fn source(&self, file: FileId) -> Option<&SourceFile> {
        self.declaration_snapshot().source(file)
    }
}

#[cfg(test)]
mod arena_tests;
#[cfg(test)]
mod associated_const_tests;
#[cfg(test)]
mod callable_tests;
#[cfg(test)]
mod completion_tests;
#[cfg(test)]
mod constructor_tests;
#[cfg(test)]
mod gat_tests;
#[cfg(test)]
mod generic_type_tests;
#[cfg(test)]
mod identity_tests;
#[cfg(test)]
mod member_tests;
#[cfg(test)]
mod namespace_tests;
#[cfg(test)]
mod owner_tests;
#[cfg(test)]
mod payload_tests;
#[cfg(test)]
mod prelude_tests;
#[cfg(test)]
mod signature_tests;
#[cfg(test)]
mod trait_catalog_tests;
#[cfg(test)]
mod trait_identity_tests;
#[cfg(test)]
mod trait_reference_tests;
#[cfg(test)]
mod type_application_tests;
#[cfg(test)]
mod type_name_tests;

#[cfg(test)]
mod tests;

impl FileAnalysis {
    /// Borrows the table that owns scoped definition IDs in this file result.
    pub fn definitions(&self) -> &DefinitionTable {
        self.result.definitions()
    }

    /// Materialize mutable authoring facts at an editor/compiler query boundary.
    pub fn to_unverified(
        &self,
        cancel: &CancellationToken,
    ) -> Result<AnalysisResult<AnalyzedModule>, DefinitionMappingError> {
        self.result.to_paths(cancel)
    }
}
