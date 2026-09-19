use kagari_common::{Diagnostic, DiagnosticKind};
use smallvec::SmallVec;

use crate::AnalysisResult;
use crate::hir::FunctionKind;
use crate::imports::{ModuleGraph, ModuleImports};
use crate::lower::LoweredModule;
use crate::resolver::resolve::BodyResolver;
use crate::resolver::table::NameTable;
use crate::resolver::{DeclarationNames, ResolvedName, ResolvedNames};

pub fn resolve_names(lowered: &LoweredModule) -> AnalysisResult<ResolvedNames> {
    let hosts = crate::host::HostDeclarations::empty();
    let graph = ModuleGraph::build([lowered], &hosts, &Default::default())
        .expect("uncancelled name resolution");
    let imports = graph
        .node(lowered.source.module_identity())
        .unwrap()
        .imports
        .clone();
    let declarations = collect_declarations(lowered, hosts, imports, &Default::default());
    AnalysisResult {
        facts: resolve_bodies(
            lowered,
            &declarations.facts,
            crate::hir::BodySelection::All,
            &Default::default(),
        ),
        diagnostics: declarations.diagnostics,
    }
}
pub(crate) fn collect_declarations(
    lowered: &LoweredModule,
    hosts: std::sync::Arc<crate::host::HostDeclarations>,
    imports: std::sync::Arc<ModuleImports>,
    cancel: &kagari_common::cancellation::CancellationToken,
) -> AnalysisResult<DeclarationNames> {
    let mut names = NameTable::default();
    let mut diagnostics = SmallVec::<[Diagnostic; 4]>::new();

    let mut declarations = Vec::new();
    for function in &lowered.module.functions {
        if cancel.check().is_err() {
            break;
        }
        if function.kind != FunctionKind::User {
            continue;
        }
        if function.name.is_empty() {
            diagnostics.push(Diagnostic::error(DiagnosticKind::MissingFunctionName));
            continue;
        }
        declarations.push((
            &function.name,
            ResolvedName::Function(function.id),
            lowered.source_map.function_span(function.id),
        ));
    }
    for item in &lowered.module.consts {
        if cancel.check().is_err() {
            break;
        }
        declarations.push((
            &item.name,
            ResolvedName::Const(item.id),
            lowered.source_map.const_span(item.id),
        ));
    }
    for item in &lowered.module.modules {
        if cancel.check().is_err() {
            break;
        }
        declarations.push((
            &item.name,
            ResolvedName::Module(item.id),
            lowered.source_map.module_span(item.id),
        ));
    }
    for item in &lowered.module.structs {
        if cancel.check().is_err() {
            break;
        }
        declarations.push((
            &item.name,
            ResolvedName::Struct(item.id),
            lowered.source_map.struct_span(item.id),
        ));
    }
    for item in &lowered.module.enums {
        if cancel.check().is_err() {
            break;
        }
        declarations.push((
            &item.name,
            ResolvedName::Enum(item.id),
            lowered.source_map.enum_span(item.id),
        ));
    }
    for item in &lowered.module.traits {
        if cancel.check().is_err() {
            break;
        }
        declarations.push((
            &item.name,
            ResolvedName::Trait(item.id),
            lowered.source_map.trait_span(item.id),
        ));
    }
    declarations.sort_by_key(|(_, _, span)| span.start);
    for (name, target, span) in declarations {
        if cancel.check().is_err() {
            break;
        }
        if !name.is_empty() && !names.insert(name.clone(), Some(target)) {
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::DuplicateDeclaration { name: name.clone() })
                    .with_span(span),
            );
        }
    }

    diagnostics.extend(imports.diagnostics.iter().cloned());
    for (index, import) in imports.entries.iter().enumerate() {
        if cancel.check().is_err() {
            break;
        }
        let target = imports.resolved_name(ResolvedName::SourceImport(index));
        names.insert(import.alias.clone(), target);
    }

    for enum_def in &lowered.module.enums {
        if cancel.check().is_err() {
            break;
        }
        let mut seen = std::collections::HashSet::new();
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
        let mut seen = std::collections::HashSet::new();
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
        names.insert_impl(impl_block.id);
        let mut seen = std::collections::HashSet::new();
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
            items: std::sync::Arc::new(names),
            hosts,
            imports,
        },
        diagnostics,
    }
}

pub(crate) fn resolve_bodies(
    lowered: &LoweredModule,
    names: &DeclarationNames,
    selection: crate::hir::BodySelection,
    cancel: &kagari_common::cancellation::CancellationToken,
) -> ResolvedNames {
    let mut resolver = BodyResolver::new(
        &names.items,
        &lowered.module,
        &lowered.source_map,
        names.hosts.clone(),
        names.imports.clone(),
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
        resolver.resolve_function(
            function.id,
            function
                .params
                .iter()
                .map(|param| (param.name.as_str(), param.id)),
            function.body,
        );
    }

    resolver.finish()
}
