use kagari_common::{Diagnostic, DiagnosticKind};

use crate::{
    AnalyzedModule, DiagnosticBuffer,
    builtin::BuiltinFunction,
    typeck::{CallTarget, ResolvedCall},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LanguageFeatureProfile {
    pub allow_reflection: bool,
    pub allow_reflection_write: bool,
    pub allow_interface_values: bool,
    pub allow_host_calls: bool,
    pub allow_path_mutation: bool,
    pub allow_module_loading: bool,
    pub allow_jit: bool,
    pub allow_eval: bool,
    pub allow_async: bool,
}

impl Default for LanguageFeatureProfile {
    fn default() -> Self {
        Self {
            allow_reflection: false,
            allow_reflection_write: false,
            allow_interface_values: true,
            allow_host_calls: false,
            allow_path_mutation: false,
            allow_module_loading: false,
            allow_jit: false,
            allow_eval: false,
            allow_async: false,
        }
    }
}

pub fn validate_profile(
    module: &AnalyzedModule,
    profile: LanguageFeatureProfile,
) -> Result<(), Box<DiagnosticBuffer>> {
    let mut diagnostics = DiagnosticBuffer::new();
    validate_reflection_calls(module, profile, &mut diagnostics);
    validate_interface_values(module, profile, &mut diagnostics);

    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(Box::new(diagnostics))
    }
}

fn validate_reflection_calls(
    module: &AnalyzedModule,
    profile: LanguageFeatureProfile,
    diagnostics: &mut DiagnosticBuffer,
) {
    for (expr_id, _) in module.lowered.module.body.expressions() {
        let Some(ResolvedCall {
            target: CallTarget::RuntimeHelper(builtin),
            ..
        }) = module.typed.type_table.call_resolution(expr_id)
        else {
            continue;
        };
        match builtin {
            BuiltinFunction::TypeOf | BuiltinFunction::GetField if !profile.allow_reflection => {
                diagnostics.push(profile_error(
                    "reflection",
                    module.lowered.source_map.expr_span(expr_id),
                ));
            }
            BuiltinFunction::SetField | BuiltinFunction::SetIndex
                if !profile.allow_reflection || !profile.allow_reflection_write =>
            {
                diagnostics.push(profile_error(
                    "reflective writes",
                    module.lowered.source_map.expr_span(expr_id),
                ));
            }
            _ => {}
        }
    }
}

fn validate_interface_values(
    module: &AnalyzedModule,
    profile: LanguageFeatureProfile,
    diagnostics: &mut DiagnosticBuffer,
) {
    if profile.allow_interface_values {
        return;
    }

    for (index, _) in module.lowered.module.body.types.iter().enumerate() {
        if module
            .typed
            .type_table
            .type_ref(crate::hir::TypeRefId::new(index))
            .is_some_and(|resolved| matches!(resolved.ty, crate::types::TypeId::Trait(_)))
        {
            diagnostics.push(profile_error(
                "interface values",
                module
                    .lowered
                    .source_map
                    .type_span(crate::hir::TypeRefId::new(index)),
            ));
        }
    }
}

fn profile_error(feature: &'static str, span: kagari_common::Span) -> Diagnostic {
    Diagnostic::error(DiagnosticKind::ProfileFeatureDisabled { feature }).with_span(span)
}
