//! Rule predicates use syntax spelling and HIR-owned identity/validity facts.

use crate::{Finding, registry::LintId};
use kagari_common::cancellation::{CancellationToken, Cancelled};
use kagari_hir::{analysis::FileAnalysis, hir::item::Item, queries::usage::BindingKind};
use kagari_source::identity::FileSpan;
use kagari_syntax::ast::{borrow::Ast, misc::ParamList, visit::NodeRef};

pub(crate) fn collect(
    file: &FileAnalysis,
    out: &mut Vec<Finding>,
    cancel: &CancellationToken,
) -> Result<(), Cancelled> {
    let facts = file.result().facts();
    let exempt: Vec<_> = facts
        .lowered
        .module
        .impls
        .iter()
        .filter(|implementation| implementation.trait_ref.is_some())
        .flat_map(|implementation| &implementation.methods)
        .filter_map(|method| {
            facts
                .lowered
                .source_map
                .item_name_span(Item::Function(method.function))
        })
        .collect();
    for node in file.syntax().descendants() {
        cancel.check()?;
        // Module traversal excludes child bodies; bodyless callable parameters
        // have their known owner here instead of recovering it through parents.
        if file.declarations_eligible() {
            match node {
                NodeRef::FnDef(function) if function.body().is_none() => {
                    declaration_parameters(file, function.param_list(), out)
                }
                NodeRef::MethodDef(method) if method.body().is_none() => {
                    declaration_parameters(file, method.param_list(), out)
                }
                _ => {}
            }
        }
        let name = match node {
            NodeRef::GenericParam(parameter) => parameter.name(),
            _ => node.declaration_name(),
        };
        let Some(name) = name else {
            continue;
        };
        let span = name.body_span();
        let id = match node {
            NodeRef::StructDef(_)
            | NodeRef::EnumDef(_)
            | NodeRef::TraitDef(_)
            | NodeRef::TypeAlias(_)
            | NodeRef::GenericParam(_)
            | NodeRef::Variant(_)
            | NodeRef::AssociatedType(_)
            | NodeRef::OpaqueTypeDef(_) => LintId::NonUpperCamelCase,
            NodeRef::ConstDef(_) | NodeRef::StaticDef(_) => LintId::NonUpperCaseGlobals,
            NodeRef::FnDef(_)
            | NodeRef::MethodDef(_)
            | NodeRef::ModuleDef(_)
            | NodeRef::Field(_)
                if file.declarations_eligible() && !exempt.contains(&span) =>
            {
                LintId::NonSnakeCase
            }
            _ => continue,
        };
        if let Some(location) = file.source().span(span) {
            naming(file, id, name.source_text(), location, out);
        }
    }

    for binding in file.binding_usage(cancel)? {
        cancel.check()?;
        let name = &binding.declaration.name;
        let location = binding.declaration.location;
        naming(file, LintId::NonSnakeCase, name, location, out);
        if binding.reads == 0 && !name.starts_with('_') && name != "self" {
            let id = match binding.kind {
                BindingKind::Local => LintId::UnusedBinding,
                BindingKind::Parameter => LintId::UnusedParameter,
            };
            push(file, id, location, format!("`{name}` is never read"), out);
        }
        if binding.kind == BindingKind::Local
            && binding.mutable
            && binding.initialized
            && binding.writes == 0
            && !binding.captured
        {
            push(
                file,
                LintId::UnnecessaryVar,
                location,
                format!("`{name}` is never rebound; use `val`"),
                out,
            );
        }
    }
    Ok(())
}

fn declaration_parameters(
    file: &FileAnalysis,
    parameters: Ast<'_, ParamList>,
    out: &mut Vec<Finding>,
) {
    for parameter in parameters.params() {
        if let Some(name) = parameter.name()
            && let Some(location) = file.source().span(name.body_span())
        {
            naming(
                file,
                LintId::NonSnakeCase,
                name.source_text(),
                location,
                out,
            );
        }
    }
}

fn naming(file: &FileAnalysis, id: LintId, name: &str, location: FileSpan, out: &mut Vec<Finding>) {
    let spelling = name.trim_start_matches('_');
    if spelling.is_empty() {
        return;
    }
    let valid = match id {
        LintId::NonSnakeCase => !spelling.chars().any(char::is_uppercase),
        LintId::NonUpperCamelCase => {
            spelling.starts_with(char::is_uppercase) && !spelling.contains('_')
        }
        LintId::NonUpperCaseGlobals => !spelling.chars().any(char::is_lowercase),
        _ => unreachable!("naming rule"),
    };
    if !valid {
        push(
            file,
            id,
            location,
            format!(
                "`{name}` does not follow {}",
                id.rule().name.trim_start_matches("non_")
            ),
            out,
        );
    }
}

fn push(file: &FileAnalysis, id: LintId, span: FileSpan, message: String, out: &mut Vec<Finding>) {
    out.push(Finding {
        id,
        span,
        module: file.source().module_identity().clone(),
        message,
        notes: vec![id.rule().limitation.into()],
    });
}
