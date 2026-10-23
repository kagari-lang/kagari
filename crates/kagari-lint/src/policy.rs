//! Optional findings receive levels here; language diagnostics never enter this policy.

use crate::{Finding, registry::LintId};
use kagari_common::{
    cancellation::{CancellationToken, Cancelled},
    identity::ModuleIdentity,
    span::Span,
};
use kagari_hir::{
    analysis::{FileAnalysis, attributes::AttributeTarget},
    lower::{AttributeFact, AttributeValue},
};
use kagari_source::{
    diagnostic::Severity, identity::FileSpan, literal::decode_string_literal,
    presentation::DiagnosticRecord,
};
use kagari_syntax::kind::SyntaxKind;
use serde::Deserialize;
use std::{
    cmp::Reverse,
    collections::{BTreeMap, HashSet},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Allow,
    Warn,
    Deny,
}

/// External levels never mutate or invalidate semantic analysis inputs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Policy {
    pub configured: BTreeMap<LintId, Level>,
    pub overrides: BTreeMap<LintId, Level>,
    pub deny_warnings: bool,
}

pub(crate) struct Scope {
    module: ModuleIdentity,
    inherited: Option<ModuleIdentity>,
    location: FileSpan,
    order: usize,
    level: Level,
    rules: Vec<LintId>,
}

pub(crate) fn scopes(
    file: &FileAnalysis,
    errors: &mut Vec<DiagnosticRecord>,
    cancel: &CancellationToken,
) -> Result<Vec<Scope>, Cancelled> {
    // The linter owns accepted policy targets; HIR supplies uninterpreted scopes.
    let mut targets = HashSet::new();
    for node in file.syntax().descendants() {
        cancel.check()?;
        if matches!(
            node.kind(),
            SyntaxKind::FnDef
                | SyntaxKind::MethodDef
                | SyntaxKind::StructDef
                | SyntaxKind::EnumDef
                | SyntaxKind::TraitDef
                | SyntaxKind::TypeAliasDef
                | SyntaxKind::ImplBlock
                | SyntaxKind::ConstDef
                | SyntaxKind::StaticDef
                | SyntaxKind::AssociatedType
                | SyntaxKind::ModuleDef
                | SyntaxKind::UseDecl
        ) {
            let range = node.text_range();
            targets.insert((usize::from(range.start()), usize::from(range.end())));
        }
    }
    let mut scopes = Vec::new();
    for scoped in file.scoped_attributes() {
        cancel.check()?;
        let attribute = scoped.attribute;
        let level = match attribute.name.as_str() {
            "tool::allow" => Level::Allow,
            "tool::warn" => Level::Warn,
            "tool::deny" => Level::Deny,
            _ => continue,
        };
        let rules = if targets.contains(&(attribute.target_span.start, attribute.target_span.end)) {
            arguments(attribute)
        } else {
            Err("lint policy requires a supported declaration target".into())
        };
        let rules = match rules {
            Ok(rules) => rules,
            Err(message) => {
                errors.push(DiagnosticRecord { severity: Severity::Error, code: "KG_LINT_POLICY".into(), span: file.source().span(attribute.span), message, notes: vec!["invalid policy was not applied; required language diagnostics are never suppressed".into()], labels: Vec::new() });
                continue;
            }
        };
        let (range, inherited) = match scoped.target {
            AttributeTarget::Declaration(range) => (range, None),
            AttributeTarget::Module {
                declaration,
                identity,
            } => (declaration, Some(identity)),
        };
        if let Some(location) = file.source().span(range) {
            scopes.push(Scope {
                module: file.source().module_identity().clone(),
                inherited,
                location,
                order: attribute.span.start,
                level,
                rules,
            });
        }
    }
    Ok(scopes)
}

fn arguments(attribute: &AttributeFact) -> Result<Vec<LintId>, String> {
    let invalid = || {
        format!(
            "{} expects one or more positional string rule IDs",
            attribute.name
        )
    };
    if attribute.value.is_some() {
        return Err(invalid());
    }
    let args = attribute
        .arguments
        .as_ref()
        .filter(|args| !args.is_empty())
        .ok_or_else(invalid)?;
    args.iter()
        .map(|arg| {
            if arg.name.is_some() {
                return Err(invalid());
            }
            let AttributeValue::Literal(value) = &arg.value else {
                return Err(invalid());
            };
            let name = decode_string_literal(value).map_err(|_| invalid())?;
            LintId::parse(&name).ok_or_else(|| format!("unknown lint rule `{name}`"))
        })
        .collect()
}

pub(crate) fn apply(
    policy: &Policy,
    scopes: &mut [Scope],
    findings: Vec<Finding>,
    cancel: &CancellationToken,
) -> Result<Vec<DiagnosticRecord>, Cancelled> {
    scopes.sort_by_key(|scope| {
        (
            scope
                .inherited
                .as_ref()
                .map_or(scope.module.path.len(), |module| module.path.len()),
            scope.inherited.is_none(),
            Reverse(scope.location.range.end - scope.location.range.start),
            scope.order,
        )
    });
    let mut records = Vec::new();
    for finding in findings {
        cancel.check()?;
        let mut level = policy
            .configured
            .get(&finding.id)
            .copied()
            .unwrap_or(finding.id.rule().default_level);
        for scope in scopes
            .iter()
            .filter(|scope| scope.rules.contains(&finding.id))
        {
            cancel.check()?;
            let inherited = scope.inherited.as_ref().is_some_and(|module| {
                module.package == finding.module.package
                    && finding.module.path.starts_with(&module.path)
            });
            let local = scope.location.file == finding.span.file
                && scope.location.revision == finding.span.revision
                && contains(scope.location.range, finding.span.range);
            if inherited || local {
                level = scope.level;
            }
        }
        level = policy.overrides.get(&finding.id).copied().unwrap_or(level);
        let severity = match level {
            Level::Allow => continue,
            Level::Warn if !policy.deny_warnings => Severity::Warning,
            Level::Warn | Level::Deny => Severity::Error,
        };
        records.push(DiagnosticRecord {
            severity,
            code: finding.id.rule().name.into(),
            span: Some(finding.span),
            message: finding.message,
            notes: finding.notes,
            labels: Vec::new(),
        });
    }
    Ok(records)
}

fn contains(outer: Span, inner: Span) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}
