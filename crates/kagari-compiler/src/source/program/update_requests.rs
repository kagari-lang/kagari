//! Candidate-scoped historical demands and routing into the ordinary module planner.

use crate::source::lower::MirLoweringError;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind,
        map::DefinitionContext,
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel},
        metadata::DefinitionMetadata,
        scoped::{DefinitionReferences, RemapDefinitions},
        table::{DefinitionTable, module::ModuleHandle},
    },
};
use kagari_contract::{
    types::{
        ConcreteFunctionIdentity, PublicItem,
        named::{NamedDeclaration, correspondence::DeclarationCorrespondence},
        proofs::{ProofCatalog, implementation::Implementation},
    },
    updates::{BodyApplication, DemandEnvelope, DemandRequests, InterfaceRequest},
};
use kagari_hir::{
    aggregates::{AggregateCatalog, implementations::ImplementationSearchLimits},
    types::{
        TypeSubstitution,
        semantic::{lower_type, raise_nominal_type, raise_type},
    },
};
use kagari_mir::verify::VerifiedMirModule;
use kagari_types::ty::{Ty, substitution::TypeTransformError};
use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
};

/// Borrow only candidate proof facts, retaining the shared identity scope that also
/// names historical caller binders. Historical declarations are not proof authority.
pub(super) fn proof_catalog<'a>(
    modules: &'a [VerifiedMirModule],
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<ProofCatalog<'a>, TypeTransformError> {
    let mut implementations = Vec::new();
    let mut contracts = Vec::new();
    for module in modules {
        cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
        for item in &module.abi.public_items {
            cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
            match item {
                PublicItem::InterfaceTable(table) => {
                    implementations.push(Implementation::Interface(table.as_ref()));
                }
                PublicItem::Trait(contract) => {
                    let owner = definitions
                        .lookup_root(&module.identity)
                        .and_then(|root| {
                            definitions.lookup_child(root, DefinitionKind::Trait, &contract.name, 0)
                        })
                        .ok_or(TypeTransformError::InvalidContract)?;
                    contracts.push((owner, contract));
                }
                _ => {}
            }
        }
        contracts.extend(
            module
                .abi
                .trait_contracts
                .iter()
                .map(|contract| (contract.declaration, &contract.abi)),
        );
    }
    ProofCatalog::new(
        implementations,
        modules.iter().flat_map(|module| &module.enumerations),
        contracts,
        modules
            .iter()
            .flat_map(|module| &module.abi.native_declarations),
        definitions,
        cancel,
    )
}

/// Runtime supplies the metadata and its semantic implementation headers together.
/// Only identities are imported here; an interned path never grants a callable target.
pub(super) fn import_retained(
    retained: &DefinitionMetadata<DemandEnvelope>,
    headers: &[NamedDeclaration],
    candidate: &[NamedDeclaration],
    context: &DefinitionContext,
    cancel: &CancellationToken,
) -> Result<DemandEnvelope, DefinitionMappingError> {
    let mut correspondence = DeclarationCorrespondence::new()?;
    let current = correspondence.import(
        &candidate.to_vec(),
        candidate.iter().cloned(),
        &context.snapshot(),
        cancel,
    )?;
    let old = correspondence.import(
        retained.records(),
        headers.iter().cloned(),
        retained.definitions(),
        cancel,
    )?;
    let mut targets = HashMap::new();
    for (source, mapped) in candidate.iter().zip(current.records()) {
        check_cancel(cancel)?;
        let entry = targets
            .entry(mapped.declaration)
            .or_insert(Some(source.declaration));
        if *entry != Some(source.declaration) {
            // Equivalent inherent blocks can split or merge. Their unambiguous
            // named children are mapped first; never pick an occurrence arbitrarily.
            *entry = None;
        }
    }
    let definitions = old.definitions();
    let mut mapped = HashMap::new();
    old.records().visit_definition_ids(
        &mut |id| {
            check_cancel(cancel)?;
            if mapped.contains_key(id) {
                return Ok(());
            }
            let path = definitions.resolve(*id)?.to_path();
            let target = if path
                .path
                .first()
                .is_some_and(|part| part.kind == DefinitionKind::Impl)
            {
                let mut ancestor = *id;
                let mut suffix = Vec::new();
                let parent = loop {
                    if let Some(Some(target)) = targets.get(&ancestor) {
                        break *target;
                    }
                    let view = definitions.resolve(ancestor)?;
                    let segment = view.last().ok_or(DefinitionMappingError::InvalidContract)?;
                    if segment.kind == DefinitionKind::Impl {
                        return Err(DefinitionMappingError::InvalidContract);
                    }
                    suffix.push(segment.to_owned());
                    ancestor = definitions
                        .parent(ancestor)?
                        .ok_or(DefinitionMappingError::InvalidContract)?;
                };
                let mut target = parent;
                for segment in suffix.into_iter().rev() {
                    target = context.intern_child(
                        target,
                        segment.kind,
                        segment.name,
                        segment.occurrence,
                    )?;
                }
                target
            } else {
                context.intern(&path)?
            };
            mapped.insert(*id, target);
            Ok(())
        },
        cancel,
    )?;
    old.records().remap_definitions(&mut DefinitionMapper::new(
        &mut |id| {
            mapped
                .get(id)
                .copied()
                .ok_or(DefinitionMappingError::InvalidContract)
        },
        &context.snapshot(),
        cancel,
    ))
}

#[derive(Default)]
pub(super) struct RequestQueue {
    modules: HashMap<ModuleHandle, DemandRequests>,
}

impl RequestQueue {
    pub fn module(&self, owner: ModuleHandle) -> Option<&DemandRequests> {
        self.modules.get(&owner)
    }

    pub fn function(
        &mut self,
        instance: ConcreteFunctionIdentity,
        definitions: &DefinitionTable,
    ) -> Result<bool, MirLoweringError> {
        let requests = self
            .modules
            .entry(
                definitions
                    .module_of(instance.declaration)
                    .map_err(DefinitionMappingError::from)?,
            )
            .or_default();
        Ok(insert(&mut requests.functions, instance))
    }

    pub fn construction(
        &mut self,
        instance: ConcreteFunctionIdentity,
        definitions: &DefinitionTable,
    ) -> Result<bool, MirLoweringError> {
        let requests = self
            .modules
            .entry(
                definitions
                    .module_of(instance.declaration)
                    .map_err(DefinitionMappingError::from)?,
            )
            .or_default();
        Ok(insert(&mut requests.constructions, instance))
    }

    pub fn shared(
        &mut self,
        application: BodyApplication,
        definitions: &DefinitionTable,
    ) -> Result<bool, MirLoweringError> {
        let owner = definitions
            .module_of(application.body.declaration)
            .map_err(DefinitionMappingError::from)?;
        Ok(insert(
            &mut self.modules.entry(owner).or_default().shared,
            application,
        ))
    }

    pub fn layouts(&mut self, layouts: HashMap<ModuleHandle, Vec<Ty>>) {
        for (owner, layouts) in layouts {
            self.modules
                .entry(owner)
                .or_default()
                .layouts
                .extend(layouts);
        }
    }

    pub fn apply(
        &mut self,
        batch: DemandRequests,
        catalog: &AggregateCatalog,
        definitions: &DefinitionTable,
        root: ModuleHandle,
        cancel: &CancellationToken,
    ) -> Result<bool, MirLoweringError> {
        let mut changed = false;
        for instance in batch.functions {
            check_cancel(cancel)?;
            changed |= self.function(instance, definitions)?;
        }
        for instance in batch.constructions {
            check_cancel(cancel)?;
            changed |= self.construction(instance, definitions)?;
        }
        for application in batch.shared {
            check_cancel(cancel)?;
            changed |= self.shared(application, definitions)?;
        }
        for ty in batch.layouts {
            check_cancel(cancel)?;
            let owner = match &ty {
                Ty::Struct(nominal) | Ty::Enum(nominal) => definitions
                    .module_of(nominal.declaration)
                    .map_err(DefinitionMappingError::from)?,
                _ => {
                    return Err(MirLoweringError::MissingBinding(
                        "retained aggregate layout",
                    ));
                }
            };
            changed |= insert(&mut self.modules.entry(owner).or_default().layouts, ty);
        }
        for request in batch.interfaces {
            check_cancel(cancel)?;
            changed |= self.interface(request, catalog, definitions, root, cancel)?;
        }
        Ok(changed)
    }

    fn interface(
        &mut self,
        request: InterfaceRequest,
        catalog: &AggregateCatalog,
        definitions: &DefinitionTable,
        root: ModuleHandle,
        cancel: &CancellationToken,
    ) -> Result<bool, MirLoweringError> {
        let mut receivers = HashSet::new();
        if let Some(receiver) = &request.receiver {
            receivers.insert(receiver.clone());
        } else {
            // Dynamic requests cover every currently admitted concrete receiver,
            // not an arbitrary specialization of an unused generic implementation.
            for implementation in catalog.implementations() {
                check_cancel(cancel)?;
                if implementation.generic_params.is_empty() {
                    receivers.insert(lower_type(&implementation.for_type));
                }
            }
            for instance in self
                .modules
                .values()
                .flat_map(|requests| &requests.constructions)
            {
                check_cancel(cancel)?;
                if let Some(implementation) =
                    catalog.implementation_signature(&instance.declaration)
                {
                    let bindings: TypeSubstitution = implementation
                        .generic_params
                        .iter()
                        .cloned()
                        .zip(instance.arguments.iter().map(raise_type))
                        .collect();
                    receivers.insert(lower_type(&implementation.for_type.instantiate(&bindings)));
                }
            }
        }
        let interface = raise_nominal_type(&request.interface);
        let mut changed = false;
        let mut found = false;
        let mut receivers = receivers.into_iter().collect::<Vec<_>>();
        // Deterministic module queues preserve function placement across hash seeds.
        let mut order_error = None;
        receivers.sort_by(|left, right| {
            left.canonical_cmp(right, definitions, cancel)
                .unwrap_or_else(|error| {
                    order_error = Some(error);
                    Ordering::Equal
                })
        });
        if let Some(error) = order_error {
            return Err(match error {
                TypeTransformError::Cancelled => MirLoweringError::Cancelled,
                _ => MirLoweringError::MissingBinding("retained receiver ordering"),
            });
        }
        for receiver in receivers {
            check_cancel(cancel)?;
            let source_receiver = raise_type(&receiver);
            let selected = catalog.concrete_interface_implementation(
                definitions,
                &interface,
                &source_receiver,
                &Default::default(),
                ImplementationSearchLimits {
                    max_checks: 100_000,
                    max_depth: 64,
                },
                cancel,
            )?;
            let owner = if let Some((declaration, _)) = selected {
                definitions
                    .module_of(declaration)
                    .map_err(DefinitionMappingError::from)?
            } else if catalog.intrinsic_implementation(
                definitions,
                &interface,
                &source_receiver,
                &Default::default(),
            ) {
                root
            } else {
                continue;
            };
            found = true;
            let concrete = InterfaceRequest {
                receiver: Some(receiver),
                ..request.clone()
            };
            changed |= insert(
                &mut self.modules.entry(owner).or_default().interfaces,
                concrete,
            );
        }
        if !found {
            return Err(MirLoweringError::MissingBinding(
                "retained interface implementation",
            ));
        }
        Ok(changed)
    }
}

#[cfg(test)]
mod tests;

fn insert<T: PartialEq>(values: &mut Vec<T>, value: T) -> bool {
    if values.contains(&value) {
        false
    } else {
        values.push(value);
        true
    }
}
