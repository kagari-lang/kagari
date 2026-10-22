//! Rule predicates use syntax spelling and HIR-owned identity/validity facts.

use crate::{Finding, registry::LintId};
use kagari_common::{
    cancellation::{CancellationToken, Cancelled},
    span::Span,
};
use kagari_hir::{
    analysis::{FileAnalysis, usage::BindingKind},
    hir::item::Item,
};
use kagari_source::identity::FileSpan;
use kagari_syntax::kind::SyntaxKind;

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
        // Inline child modules have separate analyses and policy scopes.
        if node
            .ancestors()
            .skip(1)
            .any(|parent| parent.kind() == SyntaxKind::ModuleBlock)
        {
            continue;
        }
        let Some(name) = node
            .children()
            .find(|child| child.kind() == SyntaxKind::Name)
        else {
            continue;
        };
        let Some(name) = name
            .children_with_tokens()
            .filter_map(|element| element.into_token())
            .find(|token| token.kind() == SyntaxKind::Ident)
        else {
            continue;
        };
        let range = name.text_range();
        let span = Span::new(
            u32::from(range.start()) as usize,
            u32::from(range.end()) as usize,
        );
        let id = match node.kind() {
            SyntaxKind::StructDef
            | SyntaxKind::EnumDef
            | SyntaxKind::TraitDef
            | SyntaxKind::GenericParam
            | SyntaxKind::Variant
            | SyntaxKind::AssociatedType => LintId::NonUpperCamelCase,
            SyntaxKind::ConstDef | SyntaxKind::StaticDef => LintId::NonUpperCaseGlobals,
            SyntaxKind::FnDef
            | SyntaxKind::MethodDef
            | SyntaxKind::ModuleDef
            | SyntaxKind::Field
                if file.declarations_eligible() && !exempt.contains(&span) =>
            {
                LintId::NonSnakeCase
            }
            SyntaxKind::Param
                if file.declarations_eligible()
                    && node
                        .ancestors()
                        .find(|parent| {
                            matches!(parent.kind(), SyntaxKind::FnDef | SyntaxKind::MethodDef)
                        })
                        .is_some_and(|owner| {
                            !owner
                                .children()
                                .any(|child| child.kind() == SyntaxKind::BlockExpr)
                        }) =>
            {
                LintId::NonSnakeCase
            }
            _ => continue,
        };
        if let Some(location) = file.source().span(span) {
            naming(file, id, name.text(), location, out);
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
