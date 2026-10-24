//! Compare public path languages without flattening cyclic namespace aliases.

use super::types::TypeDeclarations;
use crate::{module::VerifiedProgram, reload::ReloadValidationError};
use kagari_bytecode::{module::BytecodeModule, program::verified::VerifiedBytecodeProgram};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, ModuleIdentity,
        map::DefinitionContext,
        mapping::DefinitionMapper,
        metadata::DefinitionMetadata,
        scoped::RemapDefinitions,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_contract::{
    decode_limits::MAX_TABLE_RECORDS,
    exports::{ProgramExports, TypeExport},
    types::named::NamedContract,
};
use kagari_types::{callable::Signature, declaration::FnDecl, ty::GenericBound};
use std::collections::{BTreeMap, HashMap, HashSet};

#[cfg(test)]
mod tests;

#[derive(Default)]
struct Namespace<'a> {
    functions: BTreeMap<&'a str, usize>,
    types: BTreeMap<&'a str, TypeExport>,
    modules: BTreeMap<&'a str, usize>,
}

struct Contract {
    root: usize,
    entry: Option<usize>,
    canonical: BTreeMap<ModuleIdentity, usize>,
}

#[derive(PartialEq, Eq, Hash)]
struct FunctionContract {
    signature: Signature,
    parameters: usize,
    bounds: Vec<GenericBound>,
}

struct Comparison<'a> {
    context: DefinitionContext,
    binder: DefinitionId,
    signatures: HashMap<FunctionContract, usize>,
    namespaces: Vec<Namespace<'a>>,
}

impl<'a> Comparison<'a> {
    fn new() -> Result<Self, ReloadValidationError> {
        let context = DefinitionContext::new().map_err(|_| invalid())?;
        // Comparison-only binder: equivalent generic aliases may name different
        // declarations, but their parameter positions and constraints must agree.
        let root = context
            .intern_root(&ModuleIdentity::single_file("reload-export-signature"))
            .map_err(|_| invalid())?;
        let binder = context
            .intern_child(root, DefinitionKind::Function, "binder", 0)
            .map_err(|_| invalid())?;
        Ok(Self {
            context,
            binder,
            signatures: HashMap::new(),
            namespaces: Vec::new(),
        })
    }

    fn capture<'module>(
        &mut self,
        root: &ModuleIdentity,
        exports: &'a ProgramExports,
        definitions: &DefinitionTable,
        modules: impl Iterator<Item = &'module BytecodeModule>,
        signature: impl Fn(DefinitionId) -> Option<Signature>,
    ) -> Result<Contract, ReloadValidationError> {
        let modules = modules.collect::<Vec<_>>();
        let generic_declarations = modules
            .iter()
            .flat_map(|module| &module.named_declarations)
            .filter_map(|record| match &record.contract {
                NamedContract::Function(function) if !function.generic_params.is_empty() => {
                    Some((record.declaration, function))
                }
                _ => None,
            })
            .collect::<HashMap<_, _>>();
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
            let signature = if let Some(function) = generic_declarations.get(&id) {
                self.generic_signature(function, definitions)?
            } else {
                let signature = signature(id).ok_or_else(invalid)?;
                let signature = DefinitionMetadata::checked(
                    definitions.clone(),
                    signature,
                    &CancellationToken::default(),
                )
                .and_then(|value| value.import_into(&self.context, &CancellationToken::default()))
                .map_err(|_| invalid())?
                .into_records();
                FunctionContract {
                    signature,
                    parameters: 0,
                    bounds: Vec::new(),
                }
            };
            let next = self.signatures.len();
            let slot = *self.signatures.entry(signature).or_insert(next);
            signatures.insert(id, slot);
        }
        let declarations = TypeDeclarations::new(modules.into_iter());
        for namespace in exports.namespaces.values() {
            let types = namespace
                .types
                .iter()
                .map(|(name, export)| {
                    let target = declarations.normalize(export, definitions)?;
                    let target = DefinitionMetadata::checked(
                        definitions.clone(),
                        target,
                        &CancellationToken::default(),
                    )
                    .and_then(|value| {
                        value.import_into(&self.context, &CancellationToken::default())
                    })
                    .map_err(|_| invalid())?
                    .into_records();
                    Ok((name.as_str(), target))
                })
                .collect::<Result<_, ReloadValidationError>>()?;
            self.namespaces.push(Namespace {
                types,
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
        let canonical = if exports.namespaces.is_empty() {
            BTreeMap::from([(root.clone(), offset)])
        } else {
            indices
                .iter()
                .map(|(identity, index)| ((*identity).clone(), *index))
                .collect()
        };
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
            canonical,
        })
    }

    fn generic_signature(
        &self,
        function: &FnDecl,
        definitions: &DefinitionTable,
    ) -> Result<FunctionContract, ReloadValidationError> {
        let cancel = CancellationToken::default();
        let function = DefinitionMetadata::checked(definitions.clone(), function.clone(), &cancel)
            .and_then(|value| value.import_into(&self.context, &cancel))
            .map_err(|_| invalid())?
            .into_records();
        let owners = function
            .generic_params
            .iter()
            .map(|parameter| parameter.owner)
            .collect::<HashSet<_>>();
        let definitions = self.context.snapshot();
        let function = function
            .remap_definitions(&mut DefinitionMapper::new(
                &mut |id| {
                    Ok(if owners.contains(id) {
                        self.binder
                    } else {
                        *id
                    })
                },
                &definitions,
                &cancel,
            ))
            .map_err(|_| invalid())?;
        Ok(FunctionContract {
            signature: Signature {
                params: function
                    .params
                    .into_iter()
                    .map(|parameter| parameter.ty)
                    .collect(),
                result: function.return_type,
            },
            parameters: function.generic_params.len(),
            bounds: function.bounds,
        })
    }

    fn compare(&self, left: Contract, right: Contract) -> Result<(), ReloadValidationError> {
        if left.entry.is_some() && left.entry != right.entry {
            return Err(invalid());
        }
        let mut traversal = Preservation::new(MAX_TABLE_RECORDS);
        traversal.enqueue(left.root, right.root)?;
        // A previously exposed canonical namespace remains a query start even
        // when every root alias now reaches a different, compatible namespace.
        for (identity, left) in left.canonical {
            let right = right.canonical.get(&identity).ok_or_else(invalid)?;
            traversal.enqueue(left, *right)?;
        }
        while let Some((left, right)) = traversal.pending.pop() {
            let a = &self.namespaces[left];
            let b = &self.namespaces[right];
            for (name, signature) in &a.functions {
                traversal.charge()?;
                if b.functions.get(name) != Some(signature) {
                    return Err(invalid());
                }
            }
            for (name, ty) in &a.types {
                traversal.charge()?;
                if b.types.get(name) != Some(ty) {
                    return Err(invalid());
                }
            }
            for (name, left) in &a.modules {
                let right = b.modules.get(name).ok_or_else(invalid)?;
                traversal.enqueue(*left, *right)?;
            }
        }
        Ok(())
    }
}

/// Inclusion is directional, not an equivalence relation: joining namespaces
/// would lose obligations when one old namespace has several candidate targets.
/// Charge even repeated edges before enqueueing, bounding both work and storage.
struct Preservation {
    remaining: usize,
    seen: HashSet<(usize, usize)>,
    pending: Vec<(usize, usize)>,
}

impl Preservation {
    fn new(limit: usize) -> Self {
        Self {
            remaining: limit,
            seen: HashSet::new(),
            pending: Vec::new(),
        }
    }

    fn charge(&mut self) -> Result<(), ReloadValidationError> {
        self.remaining = self
            .remaining
            .checked_sub(1)
            .ok_or(ReloadValidationError::PublicExportComparisonLimit)?;
        Ok(())
    }

    fn enqueue(&mut self, left: usize, right: usize) -> Result<(), ReloadValidationError> {
        self.charge()?;
        if self.seen.insert((left, right)) {
            self.pending.push((left, right));
        }
        Ok(())
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
        active.modules().iter().map(|module| module.as_ref()),
        |id| active.export_signature(id),
    )?;
    let right = comparison.capture(
        &candidate.modules()[candidate.root().index()].identity,
        candidate.export_graph(),
        candidate.definitions(),
        candidate.modules().iter().map(|module| module.as_ref()),
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
        active.modules().iter().map(|module| module.as_ref()),
        |id| active.export_signature(id),
    )?;
    let program = candidate.program();
    let right = comparison.capture(
        &program.modules[program.root.index()].identity,
        &program.exports,
        candidate.definitions(),
        program.modules.iter(),
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
