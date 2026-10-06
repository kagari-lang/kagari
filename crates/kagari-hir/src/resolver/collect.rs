use crate::{
    AnalysisResult,
    hir::{ids::BodySelection, item::function::FunctionKind},
    host::HostDeclarations,
    imports::{
        BindingOrigin, ModuleGraph, ModuleImportFacts, SourceUnit, catalog::NamespaceCatalog,
    },
    lower::LoweredModule,
    resolver::{
        resolve::BodyResolver,
        resolved::{DeclarationNames, ResolvedNames},
    },
};
use smallvec::SmallVec;
use std::{collections::HashSet, sync::Arc};
use {
    kagari_common::{cancellation::CancellationToken, span::Span},
    kagari_source::diagnostic::{Diagnostic, DiagnosticKind},
};

pub fn resolve_names(lowered: &LoweredModule) -> AnalysisResult<ResolvedNames> {
    let hosts = HostDeclarations::empty();
    let graph = ModuleGraph::build([lowered], &hosts, &Default::default())
        .expect("uncancelled name resolution");
    let imports = graph.imports_for(&SourceUnit::of(lowered)).unwrap().clone();
    let declarations = collect_declarations(
        lowered,
        hosts,
        imports,
        graph.catalog.clone(),
        &Default::default(),
    );
    AnalysisResult {
        facts: resolve_bodies(
            lowered,
            &declarations.facts,
            BodySelection::All,
            &Default::default(),
        ),
        diagnostics: declarations.diagnostics,
    }
}

pub(crate) fn collect_declarations(
    lowered: &LoweredModule,
    hosts: Arc<HostDeclarations>,
    imports: Arc<ModuleImportFacts>,
    catalog: Arc<NamespaceCatalog>,
    cancel: &CancellationToken,
) -> AnalysisResult<DeclarationNames> {
    let names = imports.scope.clone();
    let mut diagnostics = SmallVec::<[Diagnostic; 4]>::new();

    for function in &lowered.module.functions {
        if cancel.check().is_err() {
            break;
        }
        if function.kind == FunctionKind::User && function.name.is_empty() {
            diagnostics.push(Diagnostic::error(DiagnosticKind::MissingFunctionName));
        }
    }
    // Declaration conflicts belong to declaration diagnostics, not graph import failure.
    for (name, entry) in &names.entries {
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
            hosts,
            imports,
        },
        diagnostics,
    }
}

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
        names.hosts.clone(),
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
    for function in &lowered.module.functions {
        if cancel.check().is_err() {
            break;
        }
        if !selection.includes(function.id) {
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
