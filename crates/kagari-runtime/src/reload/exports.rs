//! Compare public path languages without flattening cyclic namespace aliases.

use crate::{module::VerifiedProgram, reload::ReloadValidationError};
use kagari_bytecode::program::verified::VerifiedBytecodeProgram;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        ModuleIdentity,
        map::DefinitionContext,
        metadata::DefinitionMetadata,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_contract::exports::ProgramExports;
use kagari_types::callable::Signature;
use std::{
    collections::{BTreeMap, HashMap},
    mem,
};

#[derive(Default)]
struct Namespace<'a> {
    functions: BTreeMap<&'a str, usize>,
    modules: BTreeMap<&'a str, usize>,
}

struct Contract {
    root: usize,
    entry: Option<usize>,
}

struct Comparison<'a> {
    context: DefinitionContext,
    signatures: HashMap<Signature, usize>,
    namespaces: Vec<Namespace<'a>>,
}

impl<'a> Comparison<'a> {
    fn new() -> Result<Self, ReloadValidationError> {
        Ok(Self {
            context: DefinitionContext::new().map_err(|_| invalid())?,
            signatures: HashMap::new(),
            namespaces: Vec::new(),
        })
    }

    fn capture(
        &mut self,
        root: &ModuleIdentity,
        exports: &'a ProgramExports,
        definitions: &DefinitionTable,
        signature: impl Fn(DefinitionId) -> Option<Signature>,
    ) -> Result<Contract, ReloadValidationError> {
        let offset = self.namespaces.len();
        let indices = exports
            .namespaces
            .keys()
            .enumerate()
            .map(|(index, id)| (id, offset + index))
            .collect::<HashMap<_, _>>();
        let mut signatures = HashMap::new();
        // Each distinct declaration is imported once. Equal nominal spellings in
        // different tables are compared only after explicit contextual adoption.
        for id in exports
            .namespaces
            .values()
            .flat_map(|namespace| namespace.functions.values())
            .copied()
            .chain(exports.entry)
        {
            if signatures.contains_key(&id) {
                continue;
            }
            let signature = signature(id).ok_or_else(invalid)?;
            let signature = DefinitionMetadata::checked(
                definitions.clone(),
                signature,
                &CancellationToken::default(),
            )
            .and_then(|value| value.import_into(&self.context, &CancellationToken::default()))
            .map_err(|_| invalid())?
            .into_records();
            let next = self.signatures.len();
            let slot = *self.signatures.entry(signature).or_insert(next);
            signatures.insert(id, slot);
        }
        for namespace in exports.namespaces.values() {
            self.namespaces.push(Namespace {
                functions: namespace
                    .functions
                    .iter()
                    .map(|(name, id)| (name.as_str(), signatures[id]))
                    .collect(),
                modules: namespace
                    .modules
                    .iter()
                    .map(|(name, id)| {
                        indices
                            .get(id)
                            .copied()
                            .map(|index| (name.as_str(), index))
                            .ok_or_else(invalid)
                    })
                    .collect::<Result<_, _>>()?,
            });
        }
        let root = if exports.namespaces.is_empty() {
            // An explicitly empty authoring graph and an empty root namespace
            // expose the same public paths.
            self.namespaces.push(Namespace::default());
            offset
        } else {
            *indices.get(root).ok_or_else(invalid)?
        };
        Ok(Contract {
            root,
            entry: exports.entry.map(|id| signatures[&id]),
        })
    }

    fn compare(&self, left: Contract, right: Contract) -> Result<(), ReloadValidationError> {
        if left.entry != right.entry {
            return Err(invalid());
        }
        let mut sets = Equivalence::new(self.namespaces.len());
        let mut pending = vec![(left.root, right.root)];
        while let Some((left, right)) = pending.pop() {
            let left = sets.find(left);
            let right = sets.find(right);
            if left == right {
                continue;
            }
            let a = &self.namespaces[left];
            let b = &self.namespaces[right];
            if a.functions != b.functions || !a.modules.keys().eq(b.modules.keys()) {
                return Err(invalid());
            }
            sets.join(left, right);
            pending.extend(a.modules.values().copied().zip(b.modules.values().copied()));
        }
        Ok(())
    }
}

/// Union representatives have already-matched labels and signatures. Comparing
/// representatives makes each removed namespace pay for its outgoing edges once;
/// equal cycles of different lengths do not create a quadratic product traversal.
struct Equivalence {
    parents: Vec<usize>,
    sizes: Vec<usize>,
}

impl Equivalence {
    fn new(count: usize) -> Self {
        Self {
            parents: (0..count).collect(),
            sizes: vec![1; count],
        }
    }

    fn find(&mut self, mut node: usize) -> usize {
        while self.parents[node] != node {
            self.parents[node] = self.parents[self.parents[node]];
            node = self.parents[node];
        }
        node
    }

    fn join(&mut self, mut left: usize, mut right: usize) {
        if self.sizes[left] < self.sizes[right] {
            mem::swap(&mut left, &mut right);
        }
        self.parents[right] = left;
        self.sizes[left] += self.sizes[right];
    }
}

pub(super) fn runtime(
    active: &VerifiedProgram,
    candidate: &VerifiedProgram,
) -> Result<(), ReloadValidationError> {
    let mut comparison = Comparison::new()?;
    let left = comparison.capture(
        &active.modules()[active.root().index()].identity,
        active.export_graph(),
        active.definitions(),
        |id| active.export_signature(id),
    )?;
    let right = comparison.capture(
        &candidate.modules()[candidate.root().index()].identity,
        candidate.export_graph(),
        candidate.definitions(),
        |id| candidate.export_signature(id),
    )?;
    comparison.compare(left, right)
}

pub(super) fn bytecode(
    active: &VerifiedProgram,
    candidate: &VerifiedBytecodeProgram,
) -> Result<(), ReloadValidationError> {
    let mut comparison = Comparison::new()?;
    let left = comparison.capture(
        &active.modules()[active.root().index()].identity,
        active.export_graph(),
        active.definitions(),
        |id| active.export_signature(id),
    )?;
    let program = candidate.program();
    let right = comparison.capture(
        &program.modules[program.root.index()].identity,
        &program.exports,
        candidate.definitions(),
        |id| {
            let target = candidate.export_targets().get(&id)?;
            let function =
                &program.modules[target.module.index()].functions[target.function.index()];
            Some(Signature {
                params: function
                    .metadata
                    .semantic
                    .params
                    .values()
                    .cloned()
                    .collect(),
                result: function.metadata.semantic.result.clone()?,
            })
        },
    )?;
    comparison.compare(left, right)
}

fn invalid() -> ReloadValidationError {
    ReloadValidationError::PublicExportMismatch
}
