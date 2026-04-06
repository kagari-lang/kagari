use kagari_common::{Diagnostic, DiagnosticKind};
use smallvec::SmallVec;

use crate::BoxedDiagnosticBuffer;
use crate::lower::LoweredModule;
use crate::resolver::ResolvedNames;
use crate::resolver::resolve::BodyResolver;
use crate::resolver::table::NameTable;

pub fn resolve_names(lowered: &LoweredModule) -> Result<ResolvedNames, BoxedDiagnosticBuffer> {
    let mut names = NameTable::default();
    let mut diagnostics = SmallVec::<[Diagnostic; 4]>::new();

    for function in &lowered.module.functions {
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

    for enum_def in &lowered.module.enums {
        if !enum_def.name.is_empty() {
            names.insert_enum(enum_def.name.clone(), enum_def.id);
        }
    }

    if !diagnostics.is_empty() {
        return Err(Box::new(diagnostics));
    }

    let mut resolver = BodyResolver::new(&names, &lowered.module);
    for function in &lowered.module.functions {
        resolver.resolve_function(
            function
                .params
                .iter()
                .map(|param| (param.name.as_str(), param.id)),
            function.body,
        );
    }

    Ok(resolver.finish())
}
