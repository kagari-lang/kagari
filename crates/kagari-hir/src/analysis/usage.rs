//! Binding usage from resolved identities and places, shared by optional consumers.

use crate::{
    analysis::{FileAnalysis, eligibility::BodyEligibility},
    declarations::Declaration,
    hir::{
        expr::ExprKind,
        ids::{BodyOwner, HirOwner},
        place::PlaceKind,
        stmt::StmtKind,
        writeability::Writeability,
    },
    resolver::resolved::ResolvedName,
};
use kagari_common::cancellation::{CancellationToken, Cancelled};
use std::collections::HashMap;

/// Source binding category; closures use local IDs but are still parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingKind {
    Local,
    Parameter,
}

/// Usage facts for one resolved binding in a positively eligible body.
/// Writes count slot rebinding, not mutation of the referenced object's members.
#[derive(Debug)]
pub struct BindingUsage {
    pub declaration: Declaration,
    pub kind: BindingKind,
    pub reads: usize,
    pub writes: usize,
    pub captured: bool,
    pub mutable: bool,
    pub initialized: bool,
}

impl FileAnalysis {
    /// Collects eligible binding/read/write/capture facts once for this file.
    /// Shadowed bindings retain distinct identities. Unavailable bodies supply no
    /// records, so an absent read is evidence only within a returned record.
    pub fn binding_usage(
        &self,
        cancel: &CancellationToken,
    ) -> Result<Vec<BindingUsage>, Cancelled> {
        let facts = self.result().facts();
        let eligible = |owner| match owner {
            HirOwner::Body(BodyOwner::Function(function)) => {
                self.body_eligibility(function) == BodyEligibility::Complete
            }
            HirOwner::Body(BodyOwner::Static(id)) => {
                self.static_initializer_eligibility(id) == BodyEligibility::Complete
            }
            _ => false,
        };
        let mut usage = HashMap::new();
        for scope in facts.names.scopes() {
            cancel.check()?;
            if !eligible(HirOwner::Body(scope.owner)) {
                continue;
            }
            for binding in &scope.bindings {
                let kind = match binding.resolved {
                    ResolvedName::Local(_) => BindingKind::Local,
                    ResolvedName::Param(_) => BindingKind::Parameter,
                    _ => continue,
                };
                if let Some(declaration) = facts.declarations.target(binding.resolved.clone()) {
                    usage
                        .entry(binding.resolved.clone())
                        .or_insert_with(|| BindingUsage {
                            declaration: declaration.clone(),
                            kind,
                            reads: 0,
                            writes: 0,
                            captured: false,
                            mutable: false,
                            initialized: true,
                        });
                }
            }
        }
        for (id, expression) in facts.lowered.module.body.expressions() {
            cancel.check()?;
            if !eligible(id.owner()) {
                continue;
            }
            if let Some(resolved) = facts.names.expr_resolution(id)
                && let Some(binding) = usage.get_mut(&resolved)
            {
                binding.reads += 1;
            }
            if let ExprKind::Closure { params, .. } = &expression.kind {
                for param in params {
                    if let Some(binding) = usage.get_mut(&ResolvedName::Local(param.local)) {
                        binding.kind = BindingKind::Parameter;
                    }
                }
                for captured in facts.names.closure_captures(id) {
                    if let Some(binding) = usage.get_mut(captured) {
                        binding.captured = true;
                    }
                }
            }
        }
        for (id, statement) in facts.lowered.module.body.statements() {
            cancel.check()?;
            if !eligible(id.owner()) {
                continue;
            }
            match &statement.kind {
                StmtKind::Binding {
                    pattern,
                    writeability,
                    initializer,
                    ..
                } => {
                    for (local, _) in facts.lowered.module.body.pattern_bindings(*pattern) {
                        cancel.check()?;
                        if let Some(binding) = usage.get_mut(&ResolvedName::Local(local)) {
                            binding.mutable = *writeability == Writeability::Var;
                            binding.initialized = !matches!(
                                facts.lowered.module.expr(*initializer).kind,
                                ExprKind::Missing
                            );
                        }
                    }
                }
                StmtKind::Assign { target, op, .. } => {
                    let mut place = *target;
                    let mut projection = false;
                    loop {
                        cancel.check()?;
                        match facts.lowered.module.place(place).kind {
                            PlaceKind::Field { base, .. } | PlaceKind::Index { base, .. } => {
                                projection = true;
                                place = base;
                            }
                            PlaceKind::Name(_) => {
                                if let Some(resolved) = facts.names.place_resolution(place)
                                    && let Some(binding) = usage.get_mut(&resolved)
                                {
                                    if projection || op.is_some() {
                                        binding.reads += 1;
                                    }
                                    if !projection {
                                        binding.writes += 1;
                                    }
                                }
                                break;
                            }
                            PlaceKind::Expr(_) => break,
                        }
                    }
                }
                _ => {}
            }
        }
        let mut result: Vec<_> = usage.into_values().collect();
        result.sort_by_key(|binding| binding.declaration.location.range.start);
        cancel.check()?;
        Ok(result)
    }
}
