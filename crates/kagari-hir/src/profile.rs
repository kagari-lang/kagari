use std::collections::HashSet;

use kagari_common::{Diagnostic, DiagnosticKind};

use crate::{
    AnalyzedModule, DiagnosticBuffer,
    builtin::BuiltinFunction,
    hir::{ExprKind, TypeKind},
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
    for (index, expr) in module.lowered.module.body.exprs.iter().enumerate() {
        let ExprKind::Call { callee, .. } = &expr.kind else {
            continue;
        };
        let callee_expr = module.lowered.module.expr(*callee);
        let ExprKind::Name(name) = &callee_expr.kind else {
            continue;
        };
        let Some(builtin) = BuiltinFunction::from_name(name) else {
            continue;
        };
        match builtin {
            BuiltinFunction::TypeOf | BuiltinFunction::GetField if !profile.allow_reflection => {
                diagnostics.push(profile_error(
                    "reflection",
                    module
                        .lowered
                        .source_map
                        .expr_span(crate::hir::ExprId::new(index)),
                ));
            }
            BuiltinFunction::SetField | BuiltinFunction::SetIndex
                if !profile.allow_reflection || !profile.allow_reflection_write =>
            {
                diagnostics.push(profile_error(
                    "reflective writes",
                    module
                        .lowered
                        .source_map
                        .expr_span(crate::hir::ExprId::new(index)),
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

    let trait_names = module
        .lowered
        .module
        .traits
        .iter()
        .map(|trait_def| trait_def.name.as_str())
        .collect::<HashSet<_>>();
    if trait_names.is_empty() {
        return;
    }

    for (index, ty) in module.lowered.module.body.types.iter().enumerate() {
        let interface_name = match &ty.kind {
            TypeKind::Named(name) | TypeKind::Generic { name, .. }
                if trait_names.contains(name.as_str()) =>
            {
                Some(name.as_str())
            }
            _ => None,
        };
        if interface_name.is_some() {
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
