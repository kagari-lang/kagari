//! Directional protected closure comparison in an already imported comparison scope.
use crate::{
    callable::witness::OperationWitness,
    types::ConcreteFunctionIdentity,
    updates::{
        BodyRecord, CallRequirement, CurrentTarget, Dependency, ProtectedRoot, SourceRecord,
        UpdateEvidence,
        limits::{Budget, UpdateError},
    },
};
use kagari_common::identity::table::DefinitionId;
use std::collections::{HashMap, HashSet, VecDeque};

struct Index<'a> {
    sources: HashMap<DefinitionId, &'a SourceRecord>,
    bodies: HashMap<&'a ConcreteFunctionIdentity, &'a BodyRecord>,
    targets: HashMap<&'a ConcreteFunctionIdentity, &'a CurrentTarget>,
}

impl<'a> Index<'a> {
    fn new(evidence: &'a UpdateEvidence, budget: &mut Budget<'_>) -> Result<Self, UpdateError> {
        let mut index = Self {
            sources: HashMap::new(),
            bodies: HashMap::new(),
            targets: HashMap::new(),
        };
        for source in &evidence.sources {
            budget.visit()?;
            if index.sources.insert(source.declaration, source).is_some() {
                return Err(UpdateError::InvalidEvidence);
            }
        }
        for body in &evidence.bodies {
            budget.visit()?;
            if index
                .bodies
                .insert(&body.requirement.origin, body)
                .is_some()
            {
                return Err(UpdateError::InvalidEvidence);
            }
        }
        for target in &evidence.targets {
            budget.visit()?;
            if !index.bodies.contains_key(&target.body) {
                return Err(UpdateError::MissingBody(target.body.clone()));
            }
            if index.targets.insert(&target.application, target).is_some() {
                return Err(UpdateError::InvalidEvidence);
            }
        }
        Ok(index)
    }

    fn callable(&self, identity: &ConcreteFunctionIdentity) -> Result<Dependency, UpdateError> {
        if let Some(target) = self.targets.get(identity) {
            Ok(Dependency::Body(target.body.clone()))
        } else if self.bodies.contains_key(identity) {
            Ok(Dependency::Body(identity.clone()))
        } else if self.sources.contains_key(&identity.declaration) {
            // Uninstantiated source templates are protected before code exists.
            Ok(Dependency::Source(identity.declaration))
        } else {
            Err(UpdateError::MissingTarget(identity.clone()))
        }
    }
}

/// Compare every prior root, allowing unrelated new declarations only when the old
/// closure is known. Both arguments must have passed independent inventory admission
/// and must share a `DeclarationCorrespondence` scope. Import verified evidence with
/// `updates::correspondence::import_verified_evidence` to normalize equal inherent
/// impl headers after correspondence. All remaining duplicate keys reject.
/// No hashes establish equality.
/// Unknown interface/native/witness closure deliberately freezes the whole inventory;
/// absence of callbacks or effect flags is never interpreted as sealed native trust.
pub fn compare_protected(
    previous: &UpdateEvidence,
    candidate: &UpdateEvidence,
    budget: &mut Budget<'_>,
) -> Result<(), UpdateError> {
    let old = Index::new(previous, budget)?;
    let new = Index::new(candidate, budget)?;
    let mut roots = HashMap::new();
    for root in &candidate.roots {
        budget.visit()?;
        if roots.insert((&root.declaration, root.kind), root).is_some() {
            return Err(UpdateError::InvalidEvidence);
        }
    }
    for root in &previous.roots {
        budget.visit()?;
        if roots.get(&(&root.declaration, root.kind)).copied() != Some(root) {
            return Err(changed(root, None));
        }
        let mut pending = VecDeque::new();
        let mut seen = HashSet::new();
        for dependency in &root.dependencies {
            enqueue(dependency.clone(), &mut pending, &mut seen, budget)?;
        }
        let mut unknown = calls(
            &root.selections,
            (&old, &new, root),
            &mut pending,
            &mut seen,
            budget,
        )?;
        while let Some(dependency) = pending.pop_front() {
            budget.visit()?;
            let (dependencies, requirements) = match dependency {
                Dependency::Source(id) => {
                    let record = old.sources.get(&id).ok_or(UpdateError::InvalidEvidence)?;
                    if new.sources.get(&id) != Some(record) {
                        return Err(changed(root, unknown));
                    }
                    (&record.dependencies, &record.calls)
                }
                Dependency::Body(ref id) => {
                    let record = old
                        .bodies
                        .get(id)
                        .ok_or_else(|| UpdateError::MissingBody(id.clone()))?;
                    if new.bodies.get(id) != Some(record) {
                        return Err(changed(root, unknown));
                    }
                    (&record.dependencies, &record.requirement.calls)
                }
                Dependency::Unknown(edge) => {
                    unknown = Some(edge.edge);
                    continue;
                }
            };
            for dependency in dependencies {
                enqueue(dependency.clone(), &mut pending, &mut seen, budget)?;
            }
            unknown = unknown.or(calls(
                requirements,
                (&old, &new, root),
                &mut pending,
                &mut seen,
                budget,
            )?);
        }
        if let Some(edge) = unknown {
            frozen_inventory(&old, &new, previous, candidate, budget).map_err(|error| {
                if error == UpdateError::InvalidEvidence {
                    changed(root, Some(edge))
                } else {
                    error
                }
            })?;
        }
    }
    Ok(())
}

fn changed(root: &ProtectedRoot, unknown_edge: Option<u32>) -> UpdateError {
    UpdateError::ProtectedChange {
        root: root.declaration.clone(),
        unknown_edge,
    }
}

fn enqueue(
    dependency: Dependency,
    pending: &mut VecDeque<Dependency>,
    seen: &mut HashSet<Dependency>,
    budget: &mut Budget<'_>,
) -> Result<(), UpdateError> {
    budget.visit()?;
    if !seen.contains(&dependency) {
        seen.insert(dependency.clone());
        pending.push_back(dependency);
    }
    Ok(())
}

fn calls(
    requirements: &[CallRequirement],
    comparison: (&Index<'_>, &Index<'_>, &ProtectedRoot),
    pending: &mut VecDeque<Dependency>,
    seen: &mut HashSet<Dependency>,
    budget: &mut Budget<'_>,
) -> Result<Option<u32>, UpdateError> {
    let (index, candidate, root) = comparison;
    let callable = |id| {
        let dependency = index.callable(id)?;
        if candidate.callable(id)? != dependency {
            return Err(changed(root, None));
        }
        Ok(dependency)
    };
    let mut unknown = None;
    for (edge, call) in requirements.iter().enumerate() {
        budget.visit()?;
        let dependency = match call {
            CallRequirement::Direct(id) => Some(callable(id)?),
            CallRequirement::Shared(call) => Some(callable(&call.instance)?),
            CallRequirement::RetainedBody(application) => {
                Some(Dependency::Body(application.body.clone()))
            }
            CallRequirement::Operation(operation) => match operation.as_ref() {
                OperationWitness::Selected(call) => Some(callable(&call.instance)?),
                _ => {
                    unknown = Some(edge as u32);
                    None
                }
            },
            CallRequirement::Layout(_) => None,
            _ => {
                unknown = Some(edge as u32);
                None
            }
        };
        // Witnesses may invoke additional methods even when the shared entry is known.
        if matches!(call, CallRequirement::Shared(call) if !call.operations.is_empty() || !call.constructions.is_empty())
        {
            unknown = Some(edge as u32);
        }
        if let Some(dependency) = dependency {
            enqueue(dependency, pending, seen, budget)?;
        }
    }
    Ok(unknown)
}

fn frozen_inventory(
    old: &Index<'_>,
    new: &Index<'_>,
    previous: &UpdateEvidence,
    candidate: &UpdateEvidence,
    budget: &mut Budget<'_>,
) -> Result<(), UpdateError> {
    if old.bodies.len() != new.bodies.len()
        || old.sources.len() != new.sources.len()
        || old.targets.len() != new.targets.len()
        || previous.selections.len() != candidate.selections.len()
    {
        return Err(UpdateError::InvalidEvidence);
    }
    for (id, body) in &old.bodies {
        budget.visit()?;
        if new.bodies.get(id) != Some(body) {
            return Err(UpdateError::InvalidEvidence);
        }
    }
    for (id, source) in &old.sources {
        budget.visit()?;
        if new.sources.get(id) != Some(source) {
            return Err(UpdateError::InvalidEvidence);
        }
    }
    for (id, target) in &old.targets {
        budget.visit()?;
        if new
            .targets
            .get(id)
            .is_none_or(|new| new.body != target.body)
        {
            return Err(UpdateError::InvalidEvidence);
        }
    }
    // Selection order is canonical producer order, not publication placement order.
    for (old, new) in previous.selections.iter().zip(&candidate.selections) {
        budget.visit()?;
        if old != new {
            return Err(UpdateError::InvalidEvidence);
        }
    }
    Ok(())
}
