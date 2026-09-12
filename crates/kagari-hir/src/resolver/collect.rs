use kagari_common::{Diagnostic, DiagnosticKind};
use smallvec::SmallVec;

use crate::AnalysisResult;
use crate::hir::FunctionKind;
use crate::imports::{ImportTarget, ModuleGraph, ModuleImports};
use crate::lower::LoweredModule;
use crate::resolver::ResolvedNames;
use crate::resolver::resolve::BodyResolver;
use crate::resolver::table::NameTable;

pub fn resolve_names(lowered: &LoweredModule) -> AnalysisResult<ResolvedNames> {
    let hosts = crate::host::HostDeclarations::empty();
    let graph = ModuleGraph::build([lowered], &hosts, &Default::default())
        .expect("uncancelled name resolution");
    let imports = graph
        .node(lowered.source.module_identity())
        .unwrap()
        .imports
        .clone();
    resolve_names_controlled(lowered, hosts, imports, &Default::default())
}
pub(crate) fn resolve_names_controlled(
    lowered: &LoweredModule,
    hosts: std::sync::Arc<crate::host::HostDeclarations>,
    imports: std::sync::Arc<ModuleImports>,
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

    diagnostics.extend(imports.diagnostics.iter().cloned());
    for (index, import) in imports.entries.iter().enumerate() {
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
        imports,
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
