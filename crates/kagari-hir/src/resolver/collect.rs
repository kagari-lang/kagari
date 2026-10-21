//! Declaration conflict collection and selected-body resolution over shared import facts.

use crate::{
    AnalysisResult,
    analysis::error::AnalysisError,
    hir::{
        ids::{BodyOwner, BodySelection, HirOwner},
        item::function::FunctionKind,
    },
    imports::{
        BindingOrigin, ModuleGraph, ModuleImportFacts, SourceUnit, catalog::NamespaceCatalog,
    },
    lower::LoweredModule,
    resolver::{
        resolve::BodyResolver,
        resolved::{DeclarationNames, ResolvedNames},
    },
};
use kagari_types::visibility::Visibility;
use smallvec::SmallVec;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use {
    kagari_common::{
        cancellation::CancellationToken,
        identity::{map::DefinitionContext, mapping::DefinitionMappingError},
        span::Span,
    },
    kagari_source::diagnostic::{Diagnostic, DiagnosticKind},
};

/// Resolves all bodies of one lowering without installed native providers.
///
/// This convenience entrypoint builds a single-module import graph and retains
/// declaration/import diagnostics. It neither installs foundation modules nor
/// performs type checking; use analysis snapshots for a complete source input set.
pub fn resolve_names(
    lowered: &LoweredModule,
) -> Result<AnalysisResult<ResolvedNames>, AnalysisError> {
    let definitions = DefinitionContext::new().map_err(DefinitionMappingError::from)?;
    let graph = ModuleGraph::build([lowered], &definitions, &Default::default())?;
    let imports = graph
        .imports_for(&SourceUnit::of(lowered, &graph.catalog.definitions))
        .unwrap()
        .clone();
    let declarations =
        collect_declarations(lowered, imports, graph.catalog.clone(), &Default::default());
    Ok(AnalysisResult {
        facts: resolve_bodies(
            lowered,
            &declarations.facts,
            BodySelection::All,
            &Default::default(),
        ),
        diagnostics: declarations.diagnostics,
    })
}

/// Shares the graph's exact scope table and adds declaration conflicts without traversing bodies.
pub(crate) fn collect_declarations(
    lowered: &LoweredModule,
    imports: Arc<ModuleImportFacts>,
    catalog: Arc<NamespaceCatalog>,
    cancel: &CancellationToken,
) -> AnalysisResult<DeclarationNames> {
    let names = imports.scope.clone();
    let mut diagnostics = SmallVec::<[Diagnostic; 4]>::new();

    let mut local_names = HashMap::new();
    for function in &lowered.module.functions {
        if cancel.check().is_err() {
            break;
        }
        if let FunctionKind::Local { block } = function.kind {
            let span = lowered.source_map.function_span(function.id);
            let names = local_names.entry(block).or_insert_with(HashSet::new);
            if !function.name.is_empty() && !names.insert(&function.name) {
                diagnostics.push(
                    Diagnostic::error(DiagnosticKind::DuplicateDeclaration {
                        name: function.name.clone(),
                    })
                    .with_span(span),
                );
            }
            let unsupported = if function.visibility != Visibility::Private {
                Some("visibility on a local function")
            } else if matches!(block.owner(), HirOwner::Body(BodyOwner::Const(_))) {
                Some("function declarations in constant initializers")
            } else if function.body.is_none() {
                Some("local functions without a body")
            } else {
                None
            };
            if let Some(feature) = unsupported {
                diagnostics.push(
                    Diagnostic::error(DiagnosticKind::UnsupportedSyntax { feature })
                        .with_span(span),
                );
            }
        }
        if function.name.is_empty() {
            diagnostics.push(Diagnostic::error(DiagnosticKind::MissingFunctionName));
        }
    }
    // Declaration conflicts belong to declaration diagnostics, not graph import failure.
    for (name, entry) in names
        .entries
        .iter()
        .flat_map(|(name, slots)| slots.iter().map(move |(_, entry)| (name, entry)))
    {
        if cancel.check().is_err() {
            break;
        }
        if entry.strong.len() > 1
            && !entry
                .strong
                .iter()
                .any(|candidate| matches!(candidate.origin, BindingOrigin::NamedImport(_)))
        {
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::DuplicateDeclaration {
                    name: name.as_str().into(),
                })
                .with_span(
                    entry
                        .strong
                        .last()
                        .and_then(|candidate| candidate.location)
                        .map_or(Span::default(), |span| span.range),
                ),
            );
        }
    }
    diagnostics.extend(imports.diagnostics.iter().cloned());
    for enum_def in &lowered.module.enums {
        if cancel.check().is_err() {
            break;
        }
        let mut seen = HashSet::new();
        for variant in &enum_def.variants {
            if cancel.check().is_err() {
                break;
            }
            if !variant.name.is_empty() && !seen.insert(&variant.name) {
                diagnostics.push(
                    Diagnostic::error(DiagnosticKind::DuplicateVariant {
                        enum_name: enum_def.name.clone(),
                        name: variant.name.clone(),
                    })
                    .with_span(lowered.source_map.variant_span(variant.id)),
                );
            }
        }
    }

    for item in &lowered.module.traits {
        if cancel.check().is_err() {
            break;
        }
        let mut seen = HashSet::new();
        for method in &item.methods {
            if cancel.check().is_err() {
                break;
            }
            if !method.name.is_empty() && !seen.insert(&method.name) {
                diagnostics.push(
                    Diagnostic::error(DiagnosticKind::DuplicateMethod {
                        owner: item.name.clone(),
                        name: method.name.clone(),
                    })
                    .with_span(lowered.source_map.function_span(method.function)),
                );
            }
        }
    }
    for impl_block in &lowered.module.impls {
        if cancel.check().is_err() {
            break;
        }

        let mut seen = HashSet::new();
        for method in &impl_block.methods {
            if cancel.check().is_err() {
                break;
            }
            if !method.name.is_empty() && !seen.insert(&method.name) {
                diagnostics.push(
                    Diagnostic::error(DiagnosticKind::DuplicateMethod {
                        owner: "impl".into(),
                        name: method.name.clone(),
                    })
                    .with_span(lowered.source_map.function_span(method.function)),
                );
            }
        }
    }

    AnalysisResult {
        facts: DeclarationNames {
            items: names,
            catalog,
            imports,
        },
        diagnostics,
    }
}

/// Resolves selected function bodies and constant initializers against the prepared declaration environment.
pub(crate) fn resolve_bodies(
    lowered: &LoweredModule,
    names: &DeclarationNames,
    selection: BodySelection,
    cancel: &CancellationToken,
) -> ResolvedNames {
    let mut resolver = BodyResolver::new(
        &names.items,
        &lowered.module,
        &lowered.source_map,
        names.imports.clone(),
        names.catalog.clone(),
        cancel.clone(),
    );
    for const_item in &lowered.module.consts {
        if cancel.check().is_err() {
            break;
        }
        resolver.resolve_top_level_expr(const_item.id, const_item.initializer);
    }
    // Resolve only selected bodies and their lexical ancestors. Ancestors supply
    // declaration-time scopes; their bodies are not type-checked by this traversal.
    let mut required = HashSet::new();
    for function in &lowered.module.functions {
        if !selection.includes(function.id) {
            continue;
        }
        let mut id = function.id;
        while required.insert(id) {
            if cancel.check().is_err() {
                break;
            }
            let FunctionKind::Local { block } = lowered.module.functions[id.index()].kind else {
                break;
            };
            let HirOwner::Body(BodyOwner::Function(parent)) = block.owner() else {
                break;
            };
            id = parent;
        }
    }
    for function in &lowered.module.functions {
        if cancel.check().is_err() {
            break;
        }
        if !required.contains(&function.id) {
            continue;
        }
        let Some(body) = function.body else {
            continue;
        };
        resolver.resolve_function(
            function.id,
            function
                .params
                .iter()
                .map(|param| (param.name.as_str(), param.id)),
            body,
        );
    }

    resolver.finish()
}
