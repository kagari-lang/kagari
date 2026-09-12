use kagari_common::{Diagnostic, DiagnosticKind};
use smallvec::SmallVec;

use crate::AnalysisResult;
use crate::builtin::surface;
use crate::hir::FunctionKind;
use crate::lower::LoweredModule;
use crate::resolver::ResolvedNames;
use crate::resolver::resolve::BodyResolver;
use crate::resolver::table::NameTable;

pub fn resolve_names(lowered: &LoweredModule) -> AnalysisResult<ResolvedNames> {
    resolve_names_controlled(
        lowered,
        crate::host::HostDeclarations::empty(),
        &Default::default(),
    )
}

pub(crate) fn resolve_names_controlled(
    lowered: &LoweredModule,
    hosts: std::sync::Arc<crate::host::HostDeclarations>,
    cancel: &kagari_common::cancellation::CancellationToken,
) -> AnalysisResult<ResolvedNames> {
    let mut names = NameTable::default();
    let mut diagnostics = SmallVec::<[Diagnostic; 4]>::new();

    for function in &lowered.module.functions {
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

    for struct_def in &lowered.module.structs {
        if !struct_def.name.is_empty() {
            names.insert_struct(struct_def.name.clone(), struct_def.id);
        }
    }

    for module_decl in &lowered.module.modules {
        if !module_decl.name.is_empty() {
            names.insert_module(module_decl.name.clone(), module_decl.id);
        }
    }

    let mut import_names = std::collections::HashSet::new();
    let local_items = lowered
        .module
        .functions
        .iter()
        .map(|f| f.name.as_str())
        .chain(lowered.module.consts.iter().map(|item| item.name.as_str()))
        .chain(lowered.module.modules.iter().map(|item| item.name.as_str()))
        .chain(lowered.module.structs.iter().map(|item| item.name.as_str()))
        .chain(lowered.module.enums.iter().map(|item| item.name.as_str()))
        .chain(lowered.module.traits.iter().map(|item| item.name.as_str()))
        .collect::<std::collections::HashSet<_>>();
    for import in &lowered.module.imports {
        if cancel.check().is_err() {
            break;
        }
        if local_items.contains(import.alias.as_str()) || !import_names.insert(&import.alias) {
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::DuplicateImport {
                    name: import.alias.clone(),
                })
                .with_span(import.span),
            );
            continue;
        }
        if let Some(module) = surface::standard_module(&import.path) {
            names.insert_standard_module(import.alias.clone(), module.kind);
        } else if let Some(function) = import.path.rsplit_once("::").and_then(|(module, name)| {
            surface::standard_module(module)
                .and_then(|module| surface::standard_function(module.kind, name))
        }) {
            names.insert_standard_function(import.alias.clone(), function.intrinsic);
        } else if let Some(function) = hosts.resolve(&import.path) {
            if import.visibility == crate::hir::Visibility::Public {
                diagnostics.push(
                    Diagnostic::error(DiagnosticKind::UnsupportedHostReExport {
                        name: import.alias.clone(),
                    })
                    .with_span(import.span),
                );
            }
            names.host_functions.insert(import.alias.clone(), function);
        } else if let Some(module) = hosts.module(&import.path) {
            if import.visibility == crate::hir::Visibility::Public {
                diagnostics.push(
                    Diagnostic::error(DiagnosticKind::UnsupportedHostReExport {
                        name: import.alias.clone(),
                    })
                    .with_span(import.span),
                );
            }
            names.host_modules.insert(import.alias.clone(), module);
        } else {
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::UnknownName {
                    name: import.path.clone(),
                })
                .with_span(import.span),
            );
        }
    }

    for const_item in &lowered.module.consts {
        if !const_item.name.is_empty() {
            names.insert_const(const_item.name.clone(), const_item.id);
        }
    }

    for enum_def in &lowered.module.enums {
        if !enum_def.name.is_empty() {
            names.insert_enum(enum_def.name.clone(), enum_def.id);
        }
    }

    for trait_def in &lowered.module.traits {
        if !trait_def.name.is_empty() {
            names.insert_trait(trait_def.name.clone(), trait_def.id);
        }
    }

    for impl_block in &lowered.module.impls {
        names.insert_impl(impl_block.id);
    }

    let mut resolver = BodyResolver::new(
        &names,
        &lowered.module,
        &lowered.source_map,
        hosts,
        cancel.clone(),
    );
    for const_item in &lowered.module.consts {
        resolver.resolve_top_level_expr(const_item.id, const_item.initializer);
    }
    for function in &lowered.module.functions {
        resolver.resolve_function(
            function.id,
            function
                .params
                .iter()
                .map(|param| (param.name.as_str(), param.id)),
            function.body,
        );
    }

    AnalysisResult {
        facts: resolver.finish(),
        diagnostics,
    }
}
