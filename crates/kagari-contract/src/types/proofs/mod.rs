//! Bounded linked proofs over validated portable implementation and layout records.
//! This catalog borrows executable contracts; it does not reconstruct source
//! signatures or infer types. Declaration validity remains the module verifier's
//! responsibility, while this layer checks dependency-dependent obligations.

mod callables;
mod composition;
pub mod defaults;
pub mod implementation;
mod normalize;
mod ownership;
mod protocols;
mod search;
mod structural;

use crate::{layout::EnumLayout, types::proofs::implementation::Implementation};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, ModuleIdentity,
        mapping::DefinitionMappingError,
        scoped::DefinitionReferences,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_types::{
    declaration::{
        FnDecl, NativeDeclaration, TraitDef,
        verify::{DeclarationValidationError, validate_native_declarations},
    },
    host_interface::type_declaration::{HostTraitImplementationDeclaration, HostTypeDeclaration},
    ty::{
        Constraint, GenericBound, GenericParam, NominalTy, Ty, inheritance,
        substitution::{TypeSubstitution, TypeTransformError},
    },
};
use std::{
    cell::Cell,
    collections::{BTreeMap, HashSet},
    slice,
};

const MAX_IMPLEMENTATIONS: usize = 4096;
const MAX_NATIVE_DECLARATIONS: usize = 4096;
const MAX_CHECKS: usize = 100_000;
const MAX_DEPTH: usize = 64;

pub struct ProofCatalog<'a> {
    definitions: DefinitionTable,
    implementations: Vec<Implementation<'a>>,
    hosts: Vec<&'a HostTypeDeclaration>,
    enumerations: BTreeMap<NominalTy, Vec<&'a Ty>>,
    contracts: BTreeMap<DefinitionId, &'a TraitDef>,
    native_declarations: BTreeMap<DefinitionId, &'a NativeDeclaration>,
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
    /// Identity scope retained by this proof operation, separate from its proofs.
    pub fn definitions(&self) -> &DefinitionTable {
        &self.definitions
    }

    fn module(&self, id: DefinitionId) -> Result<&ModuleIdentity, TypeTransformError> {
        self.definitions
            .resolve(id)
            .map(|view| view.module())
            .map_err(|_| TypeTransformError::InvalidContract)
    }

    fn associated(
        &self,
        owner: DefinitionId,
        name: &str,
    ) -> Result<DefinitionId, TypeTransformError> {
        self.definitions
            .lookup_child(owner, DefinitionKind::AssociatedType, name, 0)
            .ok_or(TypeTransformError::InvalidContract)
    }

    pub fn trait_contract(&self, declaration: &DefinitionId) -> Option<&'a TraitDef> {
        self.contracts.get(declaration).copied()
    }

    /// Resolve checked native declaration facts for a shared entry application.
    pub fn native_declaration(&self, declaration: DefinitionId) -> Option<&'a NativeDeclaration> {
        self.native_declarations.get(&declaration).copied()
    }

    /// Check linked contracts in an immutable, already registered identity scope.
    ///
    /// Registration must collect implicit method, Host implementation and language
    /// names before taking this snapshot. Missing or foreign references reject;
    /// names alone never establish an implementation or a valid declaration.
    /// Cancellation, proof limits and invalid contracts return an error.
    pub fn new(
        implementations: Vec<Implementation<'a>>,
        hosts: Vec<&'a HostTypeDeclaration>,
        enumerations: impl IntoIterator<Item = &'a EnumLayout>,
        contracts: impl IntoIterator<Item = (DefinitionId, &'a TraitDef)>,
        native_declarations: impl IntoIterator<Item = &'a NativeDeclaration>,
        definitions: &DefinitionTable,
        cancel: &CancellationToken,
    ) -> Result<Self, TypeTransformError> {
        cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
        let copier = TypeSubstitution::default();
        let mut ids = HashSet::new();
        let mut count = 0;
        for implementation in &implementations {
            cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
            definitions
                .resolve(*implementation.declaration())
                .map_err(|_| TypeTransformError::InvalidContract)?;
            match implementation {
                Implementation::Interface(record) => {
                    validate_references(*record, definitions, cancel)?
                }
                Implementation::Native { implementation, .. } => {
                    validate_references(*implementation, definitions, cancel)?
                }
            }
            for method in implementation.methods() {
                definitions
                    .lookup_child(
                        *implementation.declaration(),
                        DefinitionKind::Method,
                        &method.name,
                        0,
                    )
                    .ok_or(TypeTransformError::InvalidContract)?;
            }
            if !ids.insert(*implementation.declaration())
                || implementation.is_bridge()
                || implementation.interface().is_none()
            {
                return Err(TypeTransformError::InvalidContract);
            }
            count += 1;
            if count > MAX_IMPLEMENTATIONS {
                return Err(TypeTransformError::LimitExceeded);
            }
            copier.apply_nominal(
                implementation.interface().expect("checked trait source"),
                cancel,
            )?;
            copier.apply(implementation.receiver(), cancel)?;
            copier.apply_bounds(implementation.bounds(), definitions, cancel)?;
        }
        let mut host_ids = HashSet::new();
        let mut unique_hosts = Vec::new();
        for host in hosts {
            validate_references(host, definitions, cancel)?;
            if !host_ids.insert(&host.id) {
                continue;
            }
            for (index, implementation) in host.trait_implementations.iter().enumerate() {
                let occurrence =
                    u32::try_from(index).map_err(|_| TypeTransformError::LimitExceeded)?;
                let id = definitions
                    .lookup_child(host.id, DefinitionKind::Impl, "", occurrence)
                    .ok_or(TypeTransformError::InvalidContract)?;
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
            validate_references(layout, definitions, cancel)?;
            let key = NominalTy {
                declaration: layout.declaration,
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
            definitions
                .resolve(id)
                .map_err(|_| TypeTransformError::InvalidContract)?;
            validate_references(contract, definitions, cancel)?;
            for method in &contract.methods {
                definitions
                    .lookup_child(id, DefinitionKind::Method, &method.name, 0)
                    .ok_or(TypeTransformError::InvalidContract)?;
            }
            cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
            if let Some(old) = declarations.insert(id, contract)
                && old != contract
            {
                return Err(TypeTransformError::InvalidContract);
            }
        }
        let mut templates = BTreeMap::new();
        for template in native_declarations {
            cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
            if templates.len() >= MAX_NATIVE_DECLARATIONS {
                return Err(TypeTransformError::LimitExceeded);
            }
            validate_references(template, definitions, cancel)?;
            let owner = definitions
                .resolve(template.declaration)
                .map_err(|_| TypeTransformError::InvalidContract)?;
            validate_native_declarations(
                slice::from_ref(template),
                owner.module(),
                definitions,
                cancel,
            )
            .map_err(|error| match error {
                DeclarationValidationError::Cancelled => TypeTransformError::Cancelled,
                _ => TypeTransformError::InvalidContract,
            })?;
            if templates.insert(template.declaration, template).is_some() {
                return Err(TypeTransformError::InvalidContract);
            }
        }
        let result = Self {
            definitions: definitions.clone(),
            implementations,
            hosts: unique_hosts,
            enumerations: payloads,
            contracts: declarations,
            native_declarations: templates,
        };
        for template in result.native_declarations.values() {
            for required in &template.callable_requirements {
                if !result.callable_requirement_valid(required)
                    || !result.holds(
                        &required.interface,
                        &required.receiver,
                        &template.function.bounds,
                        cancel,
                    )?
                {
                    return Err(TypeTransformError::InvalidContract);
                }
            }
        }
        if !result.native_defaults_valid(cancel)? {
            return Err(TypeTransformError::InvalidContract);
        }
        Ok(result)
    }

    pub fn ancestry(
        &self,
        interface: &NominalTy,
        receiver: &Ty,
        cancel: &CancellationToken,
    ) -> Result<Vec<NominalTy>, TypeTransformError> {
        validate_references(interface, &self.definitions, cancel)?;
        validate_references(receiver, &self.definitions, cancel)?;
        inheritance::trait_closure(interface, receiver, cancel, &|id| {
            self.contracts.get(id).copied()
        })
    }

    /// A protocol slot from a validated carried trait declaration.
    pub fn method(&self, interface: &DefinitionId, slot: usize) -> Option<&FnDecl> {
        self.contracts.get(interface)?.methods.get(slot)
    }

    /// Trait-owned generic parameters, separately from a method's local scope.
    pub fn parameters(&self, interface: &DefinitionId) -> Option<&[GenericParam]> {
        self.contracts
            .get(interface)
            .map(|contract| contract.generic_params.as_slice())
    }

    pub fn expand_bounds(
        &self,
        bounds: &[GenericBound],
        cancel: &CancellationToken,
    ) -> Result<Vec<GenericBound>, TypeTransformError> {
        let mut expanded =
            TypeSubstitution::default().apply_bounds(bounds, &self.definitions, cancel)?;
        for bound in &expanded {
            validate_references(bound, &self.definitions, cancel)?;
        }
        let budget = Budget::new(cancel);
        for bound in &mut expanded {
            let mut additional = Vec::new();
            for constraint in &bound.constraints {
                budget.step(0)?;
                if let Constraint::Trait(interface) = constraint {
                    for parent in self.ancestry(interface, &bound.ty, cancel)? {
                        budget.step(0)?;
                        let inherited = Constraint::Trait(parent);
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

pub fn host_application(implementation: &HostTraitImplementationDeclaration) -> NominalTy {
    NominalTy {
        declaration: implementation.trait_id,
        arguments: implementation
            .trait_arguments
            .iter()
            .map(Ty::from_host_type)
            .collect(),
        associated_types: implementation
            .associated_types
            .iter()
            .map(|output| (output.declaration, Ty::from_host_type(&output.ty)))
            .collect(),
    }
}

fn satisfies(available: &NominalTy, required: &NominalTy) -> bool {
    available.satisfies(required)
}

fn validate_references<T: DefinitionReferences>(
    record: &T,
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<(), TypeTransformError> {
    record
        .visit_definition_ids(&mut |id| Ok(definitions.resolve(*id).map(|_| ())?), cancel)
        .map_err(|error| match error {
            DefinitionMappingError::Cancelled => TypeTransformError::Cancelled,
            _ => TypeTransformError::InvalidContract,
        })
}

#[cfg(test)]
mod tests;
