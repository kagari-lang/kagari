//! Compare local static recipes, never the implementation of an ordinary callee.
use crate::{
    types::ConcreteFunctionIdentity,
    updates::{
        BodyClassification, BodyRecord, SourceRecord, UpdateEvidence,
        limits::{Budget, UpdateError},
    },
};
use kagari_common::identity::table::DefinitionId;
use std::collections::{HashMap, HashSet};

struct Index<'a> {
    sources: HashMap<DefinitionId, &'a SourceRecord>,
    bodies: HashMap<&'a ConcreteFunctionIdentity, &'a BodyRecord>,
    initializers: HashSet<DefinitionId>,
    recipes: HashMap<DefinitionId, Vec<&'a ConcreteFunctionIdentity>>,
    children: HashMap<&'a ConcreteFunctionIdentity, Vec<&'a ConcreteFunctionIdentity>>,
}

impl<'a> Index<'a> {
    fn new(evidence: &'a UpdateEvidence, budget: &mut Budget<'_>) -> Result<Self, UpdateError> {
        let mut index = Self {
            sources: HashMap::new(),
            bodies: HashMap::new(),
            initializers: HashSet::new(),
            recipes: HashMap::new(),
            children: HashMap::new(),
        };
        for source in &evidence.sources {
            budget.visit()?;
            if index.sources.insert(source.declaration, source).is_some() {
                return Err(UpdateError::InvalidEvidence);
            }
        }
        for initializer in &evidence.initializers {
            budget.visit()?;
            if !index.initializers.insert(initializer.declaration) {
                return Err(UpdateError::InvalidEvidence);
            }
        }
        for body in &evidence.bodies {
            budget.visit()?;
            let origin = &body.requirement.origin;
            if index.bodies.insert(origin, body).is_some() {
                return Err(UpdateError::InvalidEvidence);
            }
            match &body.classification {
                BodyClassification::StaticInitializer(declaration) => {
                    index.recipes.entry(*declaration).or_default().push(origin);
                }
                BodyClassification::Closure(parent)
                | BodyClassification::Async(parent)
                | BodyClassification::Resume(parent)
                | BodyClassification::Adapter(parent) => {
                    index.children.entry(parent).or_default().push(origin);
                }
                BodyClassification::Named(_) | BodyClassification::IntrinsicAdapter(_) => {}
            }
        }
        Ok(index)
    }

    fn owned(
        &self,
        declaration: DefinitionId,
        budget: &mut Budget<'_>,
    ) -> Result<HashSet<&'a ConcreteFunctionIdentity>, UpdateError> {
        let source = self
            .sources
            .get(&declaration)
            .ok_or(UpdateError::InvalidEvidence)?;
        let recipes = self
            .recipes
            .get(&declaration)
            .ok_or(UpdateError::InvalidEvidence)?;
        let mut pending = Vec::new();
        let mut owned = HashSet::new();
        for recipe in recipes {
            budget.visit()?;
            pending.push(*recipe);
        }
        while let Some(origin) = pending.pop() {
            budget.visit()?;
            if !owned.insert(origin) {
                return Err(UpdateError::InvalidEvidence);
            }
            if let Some(children) = self.children.get(origin) {
                for child in children {
                    budget.visit()?;
                    pending.push(*child);
                }
            }
        }
        // Provenance must describe this lexical computation, not a retained-call graph.
        let mut recorded = HashSet::new();
        for body in &source.bodies {
            budget.visit()?;
            if !recorded.insert(body) || !owned.contains(body) {
                return Err(UpdateError::InvalidEvidence);
            }
        }
        if recorded.len() != owned.len() {
            return Err(UpdateError::InvalidEvidence);
        }
        Ok(owned)
    }
}

/// Compare every existing static's checked expression and lexically owned executable.
/// Both publications must have passed independent inventory/executable admission and
/// share a `DeclarationCorrespondence` scope. Use `import_verified_evidence` to merge
/// corresponding inherent-header provenance before comparison. This is not a verifier.
///
/// Tokens ignore formatting/comments, but retain expression syntax. Checked references,
/// call meanings, preserved instructions and independently extracted actual instructions
/// remain exact. Ordinary callees, their generated bodies and unrelated new declarations
/// may change; neither unknown calls nor retained-body references expand ownership.
/// Authored provenance is a duplicate-free set, not executable placement order.
/// Placement and physical source manifests are not expression identity. No hash proves
/// equality. The caller's shared budget must already have charged the imported evidence.
pub fn compare_initializers(
    previous: &UpdateEvidence,
    candidate: &UpdateEvidence,
    budget: &mut Budget<'_>,
) -> Result<(), UpdateError> {
    let old = Index::new(previous, budget)?;
    let new = Index::new(candidate, budget)?;
    for initializer in &previous.initializers {
        budget.visit()?;
        let declaration = initializer.declaration;
        let changed = || UpdateError::InitializerChange { declaration };
        let source = old
            .sources
            .get(&declaration)
            .ok_or(UpdateError::InvalidEvidence)?;
        let replacement_source = new.sources.get(&declaration).ok_or_else(changed)?;
        if !new.initializers.contains(&declaration)
            || source.generic != replacement_source.generic
            || source.tokens != replacement_source.tokens
            || source.inputs != replacement_source.inputs
            || source.calls != replacement_source.calls
        {
            return Err(changed());
        }
        let owned = old.owned(declaration, budget)?;
        let replacement = new.owned(declaration, budget)?;
        if owned != replacement {
            return Err(changed());
        }
        for origin in owned {
            budget.visit()?;
            if new.bodies.get(origin) != old.bodies.get(origin) {
                return Err(changed());
            }
        }
    }
    Ok(())
}
