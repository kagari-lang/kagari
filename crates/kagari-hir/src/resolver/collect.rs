use kagari_common::{Diagnostic, DiagnosticKind};
use smallvec::SmallVec;

use crate::AnalysisResult;
use crate::hir::FunctionKind;
use crate::imports::{ImportTarget, ModuleGraph, ModuleImports};
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
        if names
            .insert_function(function.name.clone(), function.id)
            .is_some()
        {
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::DuplicateFunction {
                    name: function.name.clone(),
                })
                .with_span(lowered.source_map.function_span(function.id)),
            );
        }
    }

    let mut types = Vec::new();
    for item in &lowered.module.structs {
        if cancel.check().is_err() {
            break;
        }
        types.push((
            &item.name,
            ResolvedName::Struct(item.id),
            lowered.source_map.struct_span(item.id),
        ));
    }
    for item in &lowered.module.enums {
        if cancel.check().is_err() {
            break;
        }
        types.push((
            &item.name,
            ResolvedName::Enum(item.id),
            lowered.source_map.enum_span(item.id),
        ));
    }
    for item in &lowered.module.traits {
        if cancel.check().is_err() {
            break;
        }
        types.push((
            &item.name,
            ResolvedName::Trait(item.id),
            lowered.source_map.trait_span(item.id),
        ));
    }
    types.sort_by_key(|(_, _, span)| span.start);
    for (name, target, span) in types {
        if cancel.check().is_err() {
            break;
        }
        if !name.is_empty() && !names.insert_type(name.clone(), target) {
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::DuplicateType { name: name.clone() })
                    .with_span(span),
            );
        }
    }

    for module_decl in &lowered.module.modules {
        if cancel.check().is_err() {
            break;
        }
        if !module_decl.name.is_empty() {
            names.insert_module(module_decl.name.clone(), module_decl.id);
        }
    }

    diagnostics.extend(imports.diagnostics.iter().cloned());
    for (index, import) in imports.entries.iter().enumerate() {
        if cancel.check().is_err() {
            break;
        }
        match &import.target {
            Some(ImportTarget::StandardModule(module)) => {
                names.insert_standard_module(import.alias.clone(), *module);
            }
            Some(ImportTarget::StandardFunction(function)) => {
                names.insert_standard_function(import.alias.clone(), *function);
            }
            Some(ImportTarget::HostModule(module)) => {
                names.host_modules.insert(import.alias.clone(), *module);
            }
            Some(ImportTarget::HostFunction(function)) => {
                names.host_functions.insert(import.alias.clone(), *function);
            }
            Some(ImportTarget::Source(_)) => {
                names.source_imports.insert(import.alias.clone(), index);
            }
            None => {}
        }
    }
    for const_item in &lowered.module.consts {
        if cancel.check().is_err() {
            break;
        }
        if !const_item.name.is_empty() {
            names.insert_const(const_item.name.clone(), const_item.id);
        }
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

    for impl_block in &lowered.module.impls {
        if cancel.check().is_err() {
            break;
        }
        names.insert_impl(impl_block.id);
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
