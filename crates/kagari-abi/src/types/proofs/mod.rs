//! Bounded linked proofs over validated portable implementation and layout records.
//! This catalog borrows executable contracts; it does not reconstruct source
//! signatures or infer types. Declaration validity remains the module verifier's
//! responsibility, while this layer checks dependency-dependent obligations.
mod normalize;
mod ownership;
mod search;
mod structural;

use crate::{
    layout::EnumLayout,
    types::{
        AbiType, ConstraintAbi, FunctionAbi, GenericBoundAbi, InterfaceTableAbi, NominalAbiType,
        TraitAbi, inheritance,
        substitution::{TypeSubstitution, TypeTransformError},
    },
};
use kagari_common::{
    cancellation::CancellationToken,
    host_interface::{HostTraitImplementationDeclaration, HostTypeDeclaration},
    identity::{DefinitionId, DefinitionKind, DefinitionPathSegment},
};
use std::{
    cell::Cell,
    collections::{BTreeMap, HashSet},
};

const MAX_IMPLEMENTATIONS: usize = 4096;
const MAX_CHECKS: usize = 100_000;
const MAX_DEPTH: usize = 64;

pub struct ProofCatalog<'a> {
    tables: Vec<&'a InterfaceTableAbi>,
    hosts: Vec<&'a HostTypeDeclaration>,
    enumerations: BTreeMap<NominalAbiType, Vec<&'a AbiType>>,
    contracts: BTreeMap<DefinitionId, &'a TraitAbi>,
}

struct Budget<'a> {
    left: Cell<usize>,
    cancel: &'a CancellationToken,
}
impl<'a> Budget<'a> {
    fn new(cancel: &'a CancellationToken) -> Self {
        Self {
            left: Cell::new(MAX_CHECKS),
            cancel,
        }
    }
    fn step(&self, depth: usize) -> Result<(), TypeTransformError> {
        self.cancel
            .check()
            .map_err(|_| TypeTransformError::Cancelled)?;
        if depth >= MAX_DEPTH || self.left.get() == 0 {
            return Err(TypeTransformError::LimitExceeded);
        }
        self.left.set(self.left.get() - 1);
        Ok(())
    }
}

impl<'a> ProofCatalog<'a> {
    pub fn new(
        tables: Vec<&'a InterfaceTableAbi>,
        hosts: Vec<&'a HostTypeDeclaration>,
        enumerations: impl IntoIterator<Item = &'a EnumLayout>,
        contracts: impl IntoIterator<Item = (DefinitionId, &'a TraitAbi)>,
        cancel: &CancellationToken,
    ) -> Result<Self, TypeTransformError> {
        cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
        let copier = TypeSubstitution::default();
        let mut ids = HashSet::new();
        let mut count = 0;
        for table in &tables {
            cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
            if !ids.insert(table.declaration.clone())
                || table.host_bridge
                || table.native_bridge
                || !matches!(table.trait_type, AbiType::Trait(_))
            {
                return Err(TypeTransformError::InvalidContract);
            }
            count += 1;
            if count > MAX_IMPLEMENTATIONS {
                return Err(TypeTransformError::LimitExceeded);
            }
            copier.apply(&table.trait_type, cancel)?;
            copier.apply(&table.for_type, cancel)?;
            copier.apply_bounds(&table.bounds, cancel)?;
        }
        let mut host_ids = HashSet::new();
        let mut unique_hosts = Vec::new();
        for host in hosts {
            if !host_ids.insert(&host.id) {
                continue;
            }
            for (index, implementation) in host.trait_implementations.iter().enumerate() {
                let mut id = host.id.clone();
                id.path.push(DefinitionPathSegment {
                    kind: DefinitionKind::Impl,
                    name: String::new(),
                    occurrence: index as u32,
                });
                if !ids.insert(id) {
                    return Err(TypeTransformError::InvalidContract);
                }
                count += 1;
                if count > MAX_IMPLEMENTATIONS {
                    return Err(TypeTransformError::LimitExceeded);
                }
                copier.apply_nominal(&host_application(implementation), cancel)?;
            }
            unique_hosts.push(host);
        }
        let mut payloads = BTreeMap::new();
        for layout in enumerations {
            let key = NominalAbiType {
                declaration: layout.declaration.clone(),
                arguments: layout.arguments.clone(),
                associated_types: BTreeMap::new(),
            };
            let key = copier.apply_nominal(&key, cancel)?;
            let payload: Vec<_> = layout
                .variants
                .iter()
                .flat_map(|variant| &variant.payload)
                .collect();
            for ty in &payload {
                copier.apply(ty, cancel)?;
            }
            if let Some(old) = payloads.insert(key, payload.clone())
                && old != payload
            {
                return Err(TypeTransformError::InvalidContract);
            }
        }
        let mut declarations = BTreeMap::new();
        for (id, contract) in contracts {
            cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
            if let Some(old) = declarations.insert(id, contract)
                && old != contract
            {
                return Err(TypeTransformError::InvalidContract);
            }
        }
        Ok(Self {
            tables,
            hosts: unique_hosts,
            enumerations: payloads,
            contracts: declarations,
        })
    }

    pub fn ancestry(
        &self,
        interface: &NominalAbiType,
        receiver: &AbiType,
        cancel: &CancellationToken,
    ) -> Result<Vec<NominalAbiType>, TypeTransformError> {
        inheritance::trait_closure(interface, receiver, cancel, &|id| {
            self.contracts.get(id).copied()
        })
    }

    /// A protocol slot from a validated carried trait declaration.
    pub fn method(&self, interface: &DefinitionId, slot: usize) -> Option<&FunctionAbi> {
        self.contracts.get(interface)?.methods.get(slot)
    }

    pub fn expand_bounds(
        &self,
        bounds: &[GenericBoundAbi],
        cancel: &CancellationToken,
    ) -> Result<Vec<GenericBoundAbi>, TypeTransformError> {
        let mut expanded = TypeSubstitution::default().apply_bounds(bounds, cancel)?;
        let budget = Budget::new(cancel);
        for bound in &mut expanded {
            let mut additional = Vec::new();
            for constraint in &bound.constraints {
                budget.step(0)?;
                if let ConstraintAbi::Trait(interface) = constraint {
                    for parent in self.ancestry(interface, &bound.ty, cancel)? {
                        budget.step(0)?;
                        let inherited = ConstraintAbi::Trait(parent);
                        if !bound.constraints.contains(&inherited)
                            && !additional.contains(&inherited)
                        {
                            additional.push(inherited);
                        }
                    }
                }
            }
            bound.constraints.extend(additional);
        }
        Ok(expanded)
    }
}

pub fn host_application(implementation: &HostTraitImplementationDeclaration) -> NominalAbiType {
    NominalAbiType {
        declaration: implementation.trait_id.clone(),
        arguments: implementation
            .trait_arguments
            .iter()
            .map(AbiType::from_host_type)
            .collect(),
        associated_types: implementation
            .associated_types
            .iter()
            .map(|output| {
                (
                    output.declaration.clone(),
                    AbiType::from_host_type(&output.ty),
                )
            })
            .collect(),
    }
}

fn satisfies(available: &NominalAbiType, required: &NominalAbiType) -> bool {
    available.declaration == required.declaration
        && available.arguments == required.arguments
        && required
            .associated_types
            .iter()
            .all(|(member, ty)| available.associated_types.get(member) == Some(ty))
}

#[cfg(test)]
mod tests;
